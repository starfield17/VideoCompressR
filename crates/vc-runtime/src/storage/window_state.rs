use super::atomic_json::{read_json, recover_corrupt, write_json_atomic};
use crate::error::RuntimeError;
use crate::platform::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub const WINDOW_STATE_SCHEMA_VERSION: u32 = 1;
/// Debounce interval for persisting window geometry after move/resize.
pub const GEOMETRY_SAVE_DEBOUNCE: Duration = Duration::from_millis(750);

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct WindowGeometry {
    pub width: u32,
    pub height: u32,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub maximized: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WindowState {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub windows: BTreeMap<String, WindowGeometry>,
}

const fn default_schema_version() -> u32 {
    WINDOW_STATE_SCHEMA_VERSION
}

impl Default for WindowState {
    fn default() -> Self {
        Self { schema_version: WINDOW_STATE_SCHEMA_VERSION, windows: BTreeMap::new() }
    }
}

#[derive(Clone)]
pub struct WindowStateStore {
    paths: AppPaths,
}

impl WindowStateStore {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub fn path(&self) -> PathBuf {
        self.paths.config_dir.join("window_state.json")
    }

    pub fn load(&self) -> Result<WindowState, RuntimeError> {
        let path = self.path();
        if !path.exists() {
            return Ok(WindowState::default());
        }
        match read_json::<WindowState>(&path) {
            Ok(mut state) => {
                if state.schema_version > WINDOW_STATE_SCHEMA_VERSION {
                    return Err(RuntimeError::Config(format!(
                        "unsupported window state schema version: {}",
                        state.schema_version
                    )));
                }
                state.schema_version = WINDOW_STATE_SCHEMA_VERSION;
                Ok(state)
            }
            Err(_) => {
                recover_corrupt(&path)?;
                Ok(WindowState::default())
            }
        }
    }

    pub fn save(&self, mut state: WindowState) -> Result<(), RuntimeError> {
        state.schema_version = WINDOW_STATE_SCHEMA_VERSION;
        write_json_atomic(&self.path(), &state)
    }

    /// Load once, merge all updates, and save once.
    pub fn merge_geometries(
        &self,
        updates: HashMap<String, WindowGeometry>,
    ) -> Result<(), RuntimeError> {
        if updates.is_empty() {
            return Ok(());
        }
        let mut state = self.load()?;
        for (label, geometry) in updates {
            state.windows.insert(label, geometry);
        }
        self.save(state)
    }
}

/// In-memory window geometry cache with debounced disk persistence.
///
/// Window-event handlers must only touch this type; they must never call
/// `store.load` / `store.save` or perform filesystem I/O.
#[derive(Clone)]
pub struct WindowGeometryRuntime {
    inner: Arc<WindowGeometryInner>,
}

struct WindowGeometryInner {
    store: WindowStateStore,
    cache: Arc<Mutex<HashMap<String, WindowGeometry>>>,
    pending: Arc<Mutex<HashMap<String, WindowGeometry>>>,
    revision: Arc<AtomicU64>,
    change_tx: watch::Sender<u64>,
    change_rx: Arc<Mutex<Option<watch::Receiver<u64>>>>,
    lifecycle: Arc<Mutex<GeometryLifecycle>>,
    shutdown: CancellationToken,
    save_count: Arc<AtomicU64>,
    worker_spawn_count: Arc<AtomicU64>,
}

struct GeometryLifecycle {
    started: bool,
    shutdown: bool,
    handle: Option<JoinHandle<()>>,
}

#[derive(Clone)]
struct GeometryContext {
    store: WindowStateStore,
    pending: Arc<Mutex<HashMap<String, WindowGeometry>>>,
    save_count: Arc<AtomicU64>,
}

impl WindowGeometryRuntime {
    pub fn load(store: WindowStateStore) -> Self {
        let cache = match store.load() {
            Ok(state) => state.windows.into_iter().collect(),
            Err(_) => HashMap::new(),
        };
        let (change_tx, change_rx) = watch::channel(0_u64);
        Self {
            inner: Arc::new(WindowGeometryInner {
                store,
                cache: Arc::new(Mutex::new(cache)),
                pending: Arc::new(Mutex::new(HashMap::new())),
                revision: Arc::new(AtomicU64::new(0)),
                change_tx,
                change_rx: Arc::new(Mutex::new(Some(change_rx))),
                lifecycle: Arc::new(Mutex::new(GeometryLifecycle {
                    started: false,
                    shutdown: false,
                    handle: None,
                })),
                shutdown: CancellationToken::new(),
                save_count: Arc::new(AtomicU64::new(0)),
                worker_spawn_count: Arc::new(AtomicU64::new(0)),
            }),
        }
    }

    pub fn store(&self) -> &WindowStateStore {
        &self.inner.store
    }

    pub fn get(&self, label: &str) -> Option<WindowGeometry> {
        self.inner.cache.lock().ok().and_then(|cache| cache.get(label).cloned())
    }

    /// Update memory and notify the single debouncer worker; never spawns a task.
    pub fn note_geometry(&self, label: impl Into<String>, geometry: WindowGeometry) -> bool {
        let label = label.into();
        if let Ok(mut cache) = self.inner.cache.lock() {
            cache.insert(label.clone(), geometry.clone());
        }
        if let Ok(mut pending) = self.inner.pending.lock() {
            pending.insert(label, geometry);
        }
        let revision = self.inner.revision.fetch_add(1, Ordering::SeqCst) + 1;
        self.inner.change_tx.send_replace(revision);
        true
    }

    pub fn worker_spawn_count(&self) -> u64 {
        self.inner.worker_spawn_count.load(Ordering::SeqCst)
    }

    pub fn start(&self) -> Result<(), RuntimeError> {
        let handle = tokio::runtime::Handle::try_current().map_err(|_| {
            RuntimeError::Background("geometry worker requires an existing Tokio runtime".into())
        })?;
        let mut lifecycle = self
            .inner
            .lifecycle
            .lock()
            .map_err(|_| RuntimeError::Background("geometry lifecycle lock poisoned".into()))?;
        if lifecycle.shutdown {
            return Err(RuntimeError::Background("geometry runtime is shut down".into()));
        }
        if lifecycle.started {
            return Ok(());
        }
        let receiver = self
            .inner
            .change_rx
            .lock()
            .map_err(|_| RuntimeError::Background("geometry receiver lock poisoned".into()))?
            .take()
            .ok_or_else(|| {
                RuntimeError::Background("geometry receiver was already taken".into())
            })?;
        let context = self.context();
        let weak = Arc::downgrade(&self.inner);
        let cancel = self.inner.shutdown.clone();
        self.inner.worker_spawn_count.fetch_add(1, Ordering::SeqCst);
        lifecycle.handle = Some(handle.spawn(run_geometry_worker(weak, context, receiver, cancel)));
        lifecycle.started = true;
        Ok(())
    }

    pub fn take_pending_snapshot(&self) -> HashMap<String, WindowGeometry> {
        self.inner
            .pending
            .lock()
            .map(|mut pending| std::mem::take(&mut *pending))
            .unwrap_or_default()
    }

    pub fn save_count(&self) -> u64 {
        self.inner.save_count.load(Ordering::SeqCst)
    }

    /// Persist pending geometries immediately (intended for the worker / tests).
    pub fn flush_pending_now(&self) -> Result<(), RuntimeError> {
        self.context().flush_pending_now()
    }

    pub async fn shutdown(&self) {
        let handle = {
            let Ok(mut lifecycle) = self.inner.lifecycle.lock() else {
                return;
            };
            if lifecycle.shutdown && lifecycle.handle.is_none() {
                return;
            }
            lifecycle.shutdown = true;
            self.inner.shutdown.cancel();
            lifecycle.handle.take()
        };
        if let Some(handle) = handle {
            let _ = handle.await;
        } else {
            let context = self.context();
            let _ = tokio::task::spawn_blocking(move || context.flush_pending_now()).await;
        }
    }

    fn context(&self) -> GeometryContext {
        GeometryContext {
            store: self.inner.store.clone(),
            pending: self.inner.pending.clone(),
            save_count: self.inner.save_count.clone(),
        }
    }
}

impl Drop for WindowGeometryInner {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

impl GeometryContext {
    fn flush_pending_now(&self) -> Result<(), RuntimeError> {
        let pending = self
            .pending
            .lock()
            .map(|mut pending| std::mem::take(&mut *pending))
            .unwrap_or_default();
        if pending.is_empty() {
            return Ok(());
        }
        self.store.merge_geometries(pending)?;
        self.save_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

async fn run_geometry_worker(
    weak: std::sync::Weak<WindowGeometryInner>,
    context: GeometryContext,
    mut receiver: watch::Receiver<u64>,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                flush_geometry_context(&context).await;
                break;
            }
            changed = receiver.changed() => {
                if changed.is_err() || weak.upgrade().is_none() {
                    flush_geometry_context(&context).await;
                    break;
                }
                loop {
                    let deadline = tokio::time::sleep(GEOMETRY_SAVE_DEBOUNCE);
                    tokio::pin!(deadline);
                    tokio::select! {
                        _ = cancel.cancelled() => {
                            flush_geometry_context(&context).await;
                            return;
                        }
                        changed = receiver.changed() => {
                            if changed.is_err() {
                                flush_geometry_context(&context).await;
                                return;
                            }
                        }
                        _ = &mut deadline => {
                            flush_geometry_context(&context).await;
                            break;
                        }
                    }
                }
            }
        }
    }
}

async fn flush_geometry_context(context: &GeometryContext) {
    let context = context.clone();
    match tokio::task::spawn_blocking(move || context.flush_pending_now()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(error = %error, "window geometry save failed"),
        Err(error) => tracing::warn!(error = %error, "window geometry save join failed"),
    }
}

/// Classify which window events should update geometry or flush.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometryEventKind {
    MovedOrResized,
    CloseOrDestroyed,
    Irrelevant,
}

pub fn classify_geometry_event(kind: &str) -> GeometryEventKind {
    match kind {
        "Moved" | "Resized" => GeometryEventKind::MovedOrResized,
        "CloseRequested" | "Destroyed" => GeometryEventKind::CloseOrDestroyed,
        _ => GeometryEventKind::Irrelevant,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::paths::AppPaths;

    fn runtime() -> (tempfile::TempDir, WindowGeometryRuntime) {
        let temp = tempfile::tempdir().expect("temp");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure().expect("layout");
        let store = WindowStateStore::new(paths);
        (temp, WindowGeometryRuntime::load(store))
    }

    fn geometry(width: u32, height: u32) -> WindowGeometry {
        WindowGeometry { width, height, x: Some(1), y: Some(2), maximized: false }
    }

    #[tokio::test(start_paused = true)]
    async fn window_geometry_events_are_debounced() {
        let (_temp, runtime) = runtime();
        runtime.start().expect("start worker");
        runtime.note_geometry("main", geometry(100, 100));
        // Before debounce expires: no write yet.
        tokio::time::advance(Duration::from_millis(100)).await;
        assert_eq!(runtime.save_count(), 0);
        tokio::time::advance(GEOMETRY_SAVE_DEBOUNCE).await;
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        runtime.shutdown().await;
        assert_eq!(runtime.save_count(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn window_geometry_burst_writes_once() {
        let (_temp, runtime) = runtime();
        runtime.start().expect("start worker");
        // Simulate 10_000 events updating memory only; one worker owns the debounce timer.
        for index in 0..10_000 {
            runtime.note_geometry("main", geometry(100 + (index % 50) as u32, 200));
        }
        // Only the final generation should survive the debounce window.
        tokio::time::advance(GEOMETRY_SAVE_DEBOUNCE + Duration::from_millis(50)).await;
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        runtime.shutdown().await;
        assert!(
            runtime.save_count() <= 2,
            "expected at most 2 writes after burst, got {}",
            runtime.save_count()
        );
        assert!(runtime.save_count() >= 1, "expected at least one write after debounce");
        let loaded = runtime.store().load().expect("load");
        let main = loaded.windows.get("main").expect("main geometry");
        // Latest geometry wins (last index 9999 → width 100 + 9999%50 = 149).
        assert_eq!(main.width, 100 + (9_999 % 50) as u32);
    }

    #[tokio::test(start_paused = true)]
    async fn geometry_debouncer_starts_once_and_shutdown_flushes_pending_state() {
        let (_temp, runtime) = runtime();
        runtime.start().expect("start worker");
        runtime.start().expect("second start");
        assert_eq!(runtime.worker_spawn_count(), 1);
        runtime.note_geometry("main", geometry(1234, 777));
        runtime.shutdown().await;
        assert_eq!(runtime.save_count(), 1);
        assert_eq!(runtime.store().load().expect("load").windows["main"].width, 1234);
    }

    #[test]
    fn latest_geometry_wins() {
        let (_temp, runtime) = runtime();
        runtime.note_geometry("main", geometry(100, 100));
        runtime.note_geometry("main", geometry(900, 600));
        runtime.flush_pending_now().expect("flush");
        let loaded = runtime.store().load().expect("load");
        assert_eq!(loaded.windows.get("main").map(|g| g.width), Some(900));
    }

    #[test]
    fn irrelevant_window_events_do_not_schedule_save() {
        assert_eq!(classify_geometry_event("Focused"), GeometryEventKind::Irrelevant);
        assert_eq!(classify_geometry_event("ScaleFactorChanged"), GeometryEventKind::Irrelevant);
        assert_eq!(classify_geometry_event("Moved"), GeometryEventKind::MovedOrResized);
        assert_eq!(classify_geometry_event("Resized"), GeometryEventKind::MovedOrResized);
        assert_eq!(classify_geometry_event("CloseRequested"), GeometryEventKind::CloseOrDestroyed);
        assert_eq!(classify_geometry_event("Destroyed"), GeometryEventKind::CloseOrDestroyed);
    }

    #[test]
    fn geometry_save_failure_does_not_block_window_events() {
        // Point store at a non-writable path by using a file where a directory is expected.
        let temp = tempfile::tempdir().expect("temp");
        let file_as_root = temp.path().join("not-a-dir");
        std::fs::write(&file_as_root, b"x").expect("file");
        // AppPaths may still construct; force save against a path under a file parent fails.
        let paths = AppPaths::from_root(temp.path());
        paths.ensure().expect("layout");
        let store = WindowStateStore::new(paths);
        // Make path unwritable by replacing config dir with a file after ensure.
        let config = store.path();
        if let Some(parent) = config.parent() {
            let _ = std::fs::remove_dir_all(parent);
            let _ = std::fs::write(parent, b"blocked");
        }
        let runtime = WindowGeometryRuntime::load(store);
        // note_geometry must return immediately and never panic.
        assert!(runtime.note_geometry("main", geometry(1, 1)));
        let err = runtime.flush_pending_now();
        assert!(err.is_err(), "expected save failure");
        // Further events still work in memory.
        assert!(runtime.note_geometry("main", geometry(2, 2)));
        assert_eq!(runtime.get("main").map(|g| g.width), Some(2));
    }

    #[test]
    fn merge_geometries_writes_once() {
        let temp = tempfile::tempdir().expect("temp");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure().expect("layout");
        let store = WindowStateStore::new(paths);
        let mut updates = HashMap::new();
        updates.insert("main".into(), geometry(800, 600));
        updates.insert("queue".into(), geometry(400, 300));
        store.merge_geometries(updates).expect("merge");
        let loaded = store.load().expect("load");
        assert_eq!(loaded.windows.len(), 2);
    }
}
