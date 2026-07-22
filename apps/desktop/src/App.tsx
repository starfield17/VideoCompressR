import { useEffect, useState } from "react";
import { api } from "./api/client";
import type {
  ActivityEventDto,
  AppSettingsDto,
  BootstrapDto,
  EncoderBackend,
  PlanResponseDto,
  PreviewOptionsDto,
  PreviewResultDto,
  QueueItemDto,
  QueueSnapshotDto,
  QueueStreamMessage,
  SettingsDto,
} from "./api/generated";
import { translate, type Language } from "./i18n";

const backendValues: EncoderBackend[] = ["auto", "cpu", "nvenc", "qsv", "amf", "videotoolbox"];
const parallelBackendValues: Array<[string, string]> = [
  ["nvenc", "NVENC"],
  ["qsv", "QSV"],
  ["amf", "AMF"],
  ["videotoolbox", "VideoToolbox"],
  ["cpu", "CPU"],
];
const defaultSettings: SettingsDto = {
  codec: "hevc",
  backend: "auto",
  decodeAcceleration: "software",
  parallelEnabled: false,
  parallelBackends: [],
  ratio: null,
  minVideoKbps: 250,
  maxVideoKbps: 0,
  container: "mp4",
  audioMode: "copy",
  audioBitrate: "128k",
  copySubtitles: true,
  copyExternalSubtitles: true,
  twoPass: false,
  encoderPreset: null,
  pixelFormat: "yuv420p",
  maxrateFactor: 1.25,
  bufsizeFactor: 4,
  overwrite: false,
  recursive: false,
};

const defaultAppSettings: AppSettingsDto = {
  language: "en",
  defaultPresetName: "default_hevc",
  keepPreviewTemp: true,
  recentPaths: [],
  lastSourcePath: "",
  lastOutputDir: "",
  workdirPath: "",
  ffmpegPath: "",
  ffprobePath: "",
  logLevel: "info",
  queueTableHeaderState: "",
};

const emptySnapshot: QueueSnapshotDto = {
  state: { runState: "idle", items: [] },
  metrics: {
    totalItems: 0,
    queuedItems: 0,
    runningItems: 0,
    failedItems: 0,
    doneItems: 0,
    skippedItems: 0,
    cancelledItems: 0,
    readyItems: 0,
    completedItems: 0,
    totalDurationSec: 0,
    estimatedSavedBytes: null,
    queuePercent: 0,
    etaSec: null,
    currentItemId: null,
    currentFileName: null,
    currentFilePercent: null,
    currentSpeed: null,
  },
};

type AuxiliaryKind = "queue" | "activity" | "presets" | "settings" | "preview";

function formatDuration(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return "n/a";
  const total = Math.max(0, Math.round(value));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  return hours
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`
    : `${minutes}:${String(seconds).padStart(2, "0")}`;
}

function formatBytes(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return "n/a";
  const units = ["B", "KiB", "MiB", "GiB"];
  let amount = Math.abs(value);
  let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) {
    amount /= 1024;
    unit += 1;
  }
  return `${value < 0 ? "-" : ""}${amount.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function useRuntimeSnapshot(initial: QueueSnapshotDto | null) {
  const [snapshot, setSnapshot] = useState<QueueSnapshotDto>(initial ?? emptySnapshot);
  useEffect(() => {
    api.subscribeQueue((message: QueueStreamMessage) => {
      if (message.type === "snapshot") setSnapshot(message.data);
    }).catch(() => undefined);
  }, []);
  return [snapshot, setSnapshot] as const;
}

function App() {
  const auxiliaryWindow = new URLSearchParams(window.location.search).get("window") as AuxiliaryKind | null;
  if (auxiliaryWindow) return <AuxiliaryWindow kind={auxiliaryWindow} />;
  return <MainWindow />;
}

function MainWindow() {
  const [bootstrap, setBootstrap] = useState<BootstrapDto | null>(null);
  const [settings, setSettings] = useState<SettingsDto>(defaultSettings);
  const [appSettings, setAppSettings] = useState<AppSettingsDto>(defaultAppSettings);
  const [source, setSource] = useState("");
  const [output, setOutput] = useState("");
  const [plan, setPlan] = useState<PlanResponseDto | null>(null);
  const [snapshot, setSnapshot] = useState<QueueSnapshotDto>(emptySnapshot);
  const [activeTab, setActiveTab] = useState("basic");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [language, setLanguage] = useState<Language>("en");
  const [presetNames, setPresetNames] = useState<string[]>([]);
  const [selectedPreset, setSelectedPreset] = useState("");
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [previewOptions, setPreviewOptions] = useState<PreviewOptionsDto>({
    sampleMode: "middle",
    sampleDurationSec: 30,
    customStartSec: null,
  });
  const [previewResult, setPreviewResult] = useState<PreviewResultDto | null>(null);
  const t = (key: string, fallback: string) => translate(language, key, fallback);

  useEffect(() => {
    api.bootstrap()
      .then((value) => {
        setBootstrap(value);
        setSettings(value.settings);
        setAppSettings(value.appSettings);
        setLanguage(value.appSettings.language === "zh_cn" ? "zh_cn" : "en");
        setSource(value.appSettings.lastSourcePath);
        setOutput(value.appSettings.lastOutputDir);
        setSnapshot(value.queue);
        setSelectedPreset(value.appSettings.defaultPresetName ?? "");
      })
      .catch((error) => setMessage(errorText(error)));
    api.listPresets().then(setPresetNames).catch(() => undefined);
    api.subscribeQueue((message: QueueStreamMessage) => {
      if (message.type === "snapshot") setSnapshot(message.data);
    }).catch(() => undefined);
  }, []);

  const update = <K extends keyof SettingsDto>(key: K, value: SettingsDto[K]) =>
    setSettings((current) => ({ ...current, [key]: value }));
  const updateApp = <K extends keyof AppSettingsDto>(key: K, value: AppSettingsDto[K]) =>
    setAppSettings((current) => ({ ...current, [key]: value }));
  const metrics = snapshot.metrics;
  const tDuration = formatDuration(metrics.totalDurationSec);

  async function runPlan(addToQueue: boolean) {
    if (!source.trim()) {
      setMessage(t("gui.message.source_required", "Please select a source file or directory first."));
      return;
    }
    setBusy(true);
    setMessage("");
    try {
      const request = {
        inputPath: source,
        outputDir: output || null,
        workdir: appSettings.workdirPath || null,
        ffmpegPath: appSettings.ffmpegPath || null,
        ffprobePath: appSettings.ffprobePath || null,
        settings,
      };
      const result = addToQueue ? await api.addToQueue(request) : await api.plan(request);
      setPlan(result);
      updateApp("lastSourcePath", source);
      updateApp("lastOutputDir", output);
      updateApp("recentPaths", [source, ...appSettings.recentPaths.filter((value) => value !== source)].slice(0, 10));
    } catch (error) {
      setMessage(errorText(error));
    } finally {
      setBusy(false);
    }
  }

  async function startQueue() {
    setBusy(true);
    try {
      await api.startQueue();
    } catch (error) {
      setMessage(errorText(error));
    } finally {
      setBusy(false);
    }
  }

  async function pickFile() {
    try {
      const value = await api.pickFile();
      if (typeof value === "string") setSource(value);
    } catch (error) {
      setMessage(errorText(error));
    }
  }

  async function pickDirectory() {
    try {
      const value = await api.pickDirectory();
      if (typeof value === "string") setSource(value);
    } catch (error) {
      setMessage(errorText(error));
    }
  }

  async function pickWorkdir() {
    try {
      const value = await api.pickDirectory();
      if (typeof value === "string") updateApp("workdirPath", value);
    } catch (error) {
      setMessage(errorText(error));
    }
  }

  async function pickExecutable(key: "ffmpegPath" | "ffprobePath") {
    try {
      const value = await api.pickFile();
      if (typeof value === "string") updateApp(key, value);
    } catch (error) {
      setMessage(errorText(error));
    }
  }

  async function redetectEncoders() {
    setBusy(true);
    try {
      const value = { ...appSettings, language };
      await api.saveSettings(settings);
      await api.saveAppSettings(value);
      setAppSettings(value);
      await api.redetectEncoders();
      setMessage(t("gui.button.redetect_encoders", "Re-detect encoders"));
    } catch (error) {
      setMessage(errorText(error));
    } finally {
      setBusy(false);
    }
  }

  async function pickOutputDirectory() {
    try {
      const value = await api.pickDirectory();
      if (typeof value === "string") setOutput(value);
    } catch (error) {
      setMessage(errorText(error));
    }
  }

  async function loadPreset(name: string) {
    setSelectedPreset(name);
    if (!name) return;
    try {
      setSettings(await api.loadPreset(name));
    } catch (error) {
      setMessage(errorText(error));
    }
  }

  async function runPreview() {
    if (!source.trim()) {
      setMessage(t("gui.message.source_required", "Please select a source file or directory first."));
      return;
    }
    setBusy(true);
    try {
      const result = await api.preview({
        inputPath: source,
        outputDir: output || null,
        workdir: appSettings.workdirPath || null,
        ffmpegPath: appSettings.ffmpegPath || null,
        ffprobePath: appSettings.ffprobePath || null,
        settings,
      }, previewOptions);
      setPreviewResult(result);
      try {
        localStorage.setItem("video-compressor.preview-result", JSON.stringify(result));
      } catch {
        // The result remains visible in the main window when storage is unavailable.
      }
      await api.openAuxiliary("preview");
    } catch (error) {
      setMessage(errorText(error));
    } finally {
      setBusy(false);
    }
  }

  async function saveAppSettings() {
    try {
      const value = { ...appSettings, language };
      await api.saveSettings(settings);
      await api.saveAppSettings(value);
      setAppSettings(value);
      setMessage(t("gui.message.settings_saved", "Settings saved."));
    } catch (error) {
      setMessage(errorText(error));
    }
  }

  async function openAuxiliary(kind: AuxiliaryKind) {
    try {
      await api.openAuxiliary(kind);
    } catch (error) {
      setMessage(errorText(error));
    }
  }

  const runState = snapshot.state.runState;
  const isRunning = runState === "running" || runState === "pause_requested" || runState === "cancelling";
  const selectedRows = snapshot.state.items.filter((item) => selectedIds.includes(item.itemId));
  const selectedRetryIds = selectedRows.filter((item) => item.status === "failed" || item.status === "cancelled").map((item) => item.itemId);
  const currentItem = metrics.currentItemId
    ? snapshot.state.items.find((item) => item.itemId === metrics.currentItemId)
    : undefined;
  const planReady = plan?.items.filter((item) => !item.skipReason).length ?? 0;
  const planSkipped = plan?.items.filter((item) => item.skipReason).length ?? 0;

  return (
    <div className="app-shell">
      <header className="toolbar">
        <button onClick={pickFile}>＋ {t("gui.button.add_files", "Add Files")}</button>
        <button onClick={pickDirectory}>▣ {t("gui.button.add_folder", "Add Folder")}</button>
        <button disabled={busy} onClick={() => runPlan(false)}>◇ {t("gui.button.plan", "Plan")}</button>
        <button disabled={busy} onClick={() => runPlan(true)}>▤ {t("gui.button.add_to_queue", "Add to Queue")}</button>
        <button disabled={busy || !source} onClick={runPreview}>◇ {t("gui.button.preview", "Preview")}</button>
        <button disabled={busy || metrics.queuedItems === 0} onClick={startQueue}>▶ {t("gui.button.start_queue", "Start Queue")}</button>
        <button disabled={!isRunning} onClick={() => api.pauseAfterCurrent().catch((error) => setMessage(errorText(error)))}>Ⅱ {t("gui.button.pause_after_current", "Pause After Current")}</button>
        <button disabled={!isRunning} onClick={() => api.stopQueue().catch((error) => setMessage(errorText(error)))}>■ {t("gui.button.stop", "Stop")}</button>
        <span className="toolbar-separator" />
        <button onClick={() => openAuxiliary("queue")}>☷ {t("gui.button.queue", "Queue")}</button>
        <button onClick={() => openAuxiliary("activity")}>≡ {t("gui.button.activity_log", "Activity Log")}</button>
        <button onClick={() => openAuxiliary("presets")}>▣ {t("gui.button.presets", "Presets")}</button>
        <button onClick={() => openAuxiliary("settings")}>⚙ {t("gui.button.settings", "Settings")}</button>
      </header>

      <main className="content-scroll">
        <section className="card source-card">
          <h2>{t("gui.group.source", "Source Setup")}</h2>
          <div className="grid source-grid">
            <label>{t("gui.label.source", "Source")}<input list="recent-sources" value={source} onChange={(event) => setSource(event.target.value)} placeholder={t("gui.placeholder.source", "Select a source file or directory")} /></label>
            <datalist id="recent-sources">{appSettings.recentPaths.map((value) => <option key={value} value={value} />)}</datalist>
            <div className="source-buttons"><button onClick={pickFile}>{t("gui.button.browse_file", "Pick File")}</button><button onClick={pickDirectory}>{t("gui.button.browse_dir", "Pick Dir")}</button></div>
            <label>{t("gui.label.output", "Output")}<input value={output} onChange={(event) => setOutput(event.target.value)} placeholder={t("gui.placeholder.default_output", "Leave blank to use the default output root")} /></label>
            <button onClick={pickOutputDirectory}>{t("gui.button.browse_dir", "Pick Dir")}</button>
            <label>{t("gui.label.preset", "Preset")}<select value={selectedPreset} onChange={(event) => loadPreset(event.target.value)}><option value="">{t("gui.value.default", "(default)")}</option>{presetNames.map((name) => <option key={name} value={name}>{name}</option>)}</select></label>
            <button onClick={() => openAuxiliary("presets")}>{t("gui.button.manage_presets", "Manage Presets...")}</button>
            {plan && <p className="plan-note">{t("gui.summary.plan", "Items: {total}, ready: {ready}, skipped: {skipped}").replace("{total}", String(plan.items.length)).replace("{ready}", String(planReady)).replace("{skipped}", String(planSkipped))} · {plan.outputRoot}</p>}
          </div>
        </section>

        <section className="tabs-card">
          <nav className="tabs">{[
            ["basic", "gui.tab.basic", "Basic"],
            ["video", "gui.tab.video", "Video"],
            ["audio", "gui.tab.audio_subtitles", "Audio / Subtitles"],
            ["preview", "gui.tab.preview", "Preview"],
            ["advanced", "gui.tab.advanced", "Advanced"],
          ].map(([key, translationKey, fallback]) => <button className={activeTab === key ? "active" : ""} key={key} onClick={() => setActiveTab(key)}>{t(translationKey, fallback)}</button>)}</nav>
          {activeTab === "basic" && <BasicOptions settings={settings} update={update} t={t} />}
          {activeTab === "video" && <VideoOptions settings={settings} update={update} t={t} />}
          {activeTab === "audio" && <AudioOptions settings={settings} update={update} t={t} />}
          {activeTab === "preview" && <PreviewOptions options={previewOptions} setOptions={setPreviewOptions} onPreview={runPreview} disabled={!source || busy} t={t} />}
          {activeTab === "advanced" && <AdvancedOptions appSettings={appSettings} settings={settings} update={update} updateApp={updateApp} onSave={saveAppSettings} onPickWorkdir={pickWorkdir} onPickFfmpeg={() => pickExecutable("ffmpegPath")} onPickFfprobe={() => pickExecutable("ffprobePath")} onRedetect={redetectEncoders} t={t} />}
        </section>

        <section className="card jobs-card">
          <div className="summary-grid">
            <Summary label={t("gui.summary.total_items", "Total items")} value={String(metrics.totalItems)} />
            <Summary label={t("gui.summary.total_duration", "Total duration")} value={tDuration} />
            <Summary label={t("gui.summary.states", "States")} value={`Ready ${metrics.queuedItems} / Running ${metrics.runningItems} / Failed ${metrics.failedItems}`} />
            <Summary label={t("gui.summary.estimated_saved", "Estimated saved space")} value={formatBytes(metrics.estimatedSavedBytes)} />
          </div>
          <p className="queue-progress">Queue: {metrics.queuePercent.toFixed(1)}% ({metrics.completedItems}/{metrics.totalItems}) · ETA: {formatDuration(metrics.etaSec)}</p>
          <progress max="100" value={metrics.queuePercent} />
          <QueueTable items={snapshot.state.items} selected={selectedIds} onSelect={(id, checked) => setSelectedIds((current) => checked ? [...current, id] : current.filter((value) => value !== id))} t={t} />
          <div className="inline-actions"><button disabled={selectedRetryIds.length === 0} onClick={() => api.queueRetry(selectedRetryIds).catch((error) => setMessage(errorText(error)))}>{t("gui.button.retry", "Retry selected")}</button><button disabled={selectedRows.length === 0} onClick={() => api.removeQueue(selectedRows.map((item) => item.itemId)).catch((error) => setMessage(errorText(error)))}>{t("gui.button.remove", "Remove")}</button><button disabled={metrics.totalItems === 0} onClick={() => api.clearCompleted().catch((error) => setMessage(errorText(error)))}>{t("gui.button.clear_completed", "Clear completed")}</button></div>
        </section>
      </main>

      <footer className="statusbar"><span>{t("gui.statusbar.stage", "Stage")}: {isRunning ? runState : "-"}</span><span className="status-file">{t("gui.statusbar.file", "File")}: {(metrics.currentFileName ?? source) || "-"}</span><span>{t("gui.statusbar.speed", "Speed")}: {metrics.currentSpeed ?? "-"}</span><span>{t("gui.statusbar.elapsed", "Elapsed")}: {formatDuration(currentItem?.progress.elapsedSec)}</span><span>{t("gui.statusbar.current_progress", "Current")}: {metrics.currentFilePercent == null ? "-" : `${metrics.currentFilePercent.toFixed(1)}%`}</span><progress max="100" value={metrics.currentFilePercent ?? 0} /></footer>
      {message && <div className="toast" role="alert">{message}<button onClick={() => setMessage("")}>×</button></div>}
      {previewResult && <div className="preview-summary" role="status"><strong>{t("gui.preview.result", "Preview result")}</strong><span>{previewResult.encodedSamplePath}</span><button onClick={() => setPreviewResult(null)}>×</button></div>}
      {bootstrap && <span className="sr-only">{bootstrap.ffmpegPath ?? "FFmpeg not configured"}</span>}
    </div>
  );
}

function BasicOptions({ settings, update, t }: { settings: SettingsDto; update: <K extends keyof SettingsDto>(key: K, value: SettingsDto[K]) => void; t: (key: string, fallback: string) => string }) {
  return <div className="grid option-grid">
    <label>{t("gui.label.codec", "Codec")}<select value={settings.codec} onChange={(event) => update("codec", event.target.value)}><option value="hevc">hevc</option><option value="av1">av1</option></select></label>
    <label>{t("gui.label.backend", "Backend")}<select value={settings.backend} disabled={settings.parallelEnabled} onChange={(event) => update("backend", event.target.value)}>{backendValues.map((value) => <option key={value} value={value}>{value}</option>)}</select></label>
    <label>{t("gui.label.container", "Container")}<select value={settings.container} onChange={(event) => update("container", event.target.value)}><option value="mkv">mkv</option><option value="mp4">mp4</option></select></label>
    <label>{t("gui.label.ratio", "Ratio")}<input type="number" step="0.01" value={settings.ratio ?? ""} onChange={(event) => update("ratio", event.target.value ? Number(event.target.value) : null)} placeholder="codec default" /></label>
    <label className="check"><input type="checkbox" checked={settings.overwrite} onChange={(event) => update("overwrite", event.target.checked)} />{t("gui.label.overwrite", "Overwrite")}</label>
    <label className="check"><input type="checkbox" checked={settings.recursive} onChange={(event) => update("recursive", event.target.checked)} />{t("gui.label.recursive", "Recursive")}</label>
    <label className="check"><input type="checkbox" checked={settings.parallelEnabled} onChange={(event) => update("parallelEnabled", event.target.checked)} />{t("gui.label.parallel", "Enable parallel transcoding")}</label>
    {settings.parallelEnabled && <fieldset className="parallel-options"><legend>{t("gui.label.parallel_backends", "Parallel backends")}</legend><div className="check-grid">{parallelBackendValues.map(([value, label]) => <label className="check" key={value}><input type="checkbox" checked={settings.parallelBackends.includes(value)} onChange={(event) => update("parallelBackends", event.target.checked ? [...settings.parallelBackends, value] : settings.parallelBackends.filter((current) => current !== value))} />{label}</label>)}</div></fieldset>}
  </div>;
}

function VideoOptions({ settings, update, t }: { settings: SettingsDto; update: <K extends keyof SettingsDto>(key: K, value: SettingsDto[K]) => void; t: (key: string, fallback: string) => string }) {
  return <div className="grid option-grid">
    <label>{t("gui.label.encoder_preset", "Encoder preset")}<input value={settings.encoderPreset ?? ""} onChange={(event) => update("encoderPreset", event.target.value || null)} placeholder="encoder default" /></label>
    <label>{t("gui.label.decode_acceleration", "Decode acceleration")}<select value={settings.decodeAcceleration} onChange={(event) => update("decodeAcceleration", event.target.value)}><option value="software">Software</option><option value="videotoolbox">VideoToolbox</option></select></label>
    <label>{t("gui.label.pix_fmt", "Pixel format")}<input value={settings.pixelFormat} onChange={(event) => update("pixelFormat", event.target.value)} /></label>
    <label>{t("gui.label.min_video_kbps", "Min video kbps")}<input type="number" value={settings.minVideoKbps} onChange={(event) => update("minVideoKbps", Number(event.target.value))} /></label>
    <label>{t("gui.label.max_video_kbps", "Max video kbps")}<input type="number" value={settings.maxVideoKbps} onChange={(event) => update("maxVideoKbps", Number(event.target.value))} /></label>
    <label>{t("gui.label.maxrate_factor", "Maxrate factor")}<input type="number" step="0.05" value={settings.maxrateFactor} onChange={(event) => update("maxrateFactor", Number(event.target.value))} /></label>
    <label>{t("gui.label.bufsize_factor", "Bufsize factor")}<input type="number" step="0.1" value={settings.bufsizeFactor} onChange={(event) => update("bufsizeFactor", Number(event.target.value))} /></label>
    <label className="check"><input type="checkbox" checked={settings.twoPass} onChange={(event) => update("twoPass", event.target.checked)} />{t("gui.label.two_pass", "Two-pass")}</label>
  </div>;
}

function AudioOptions({ settings, update, t }: { settings: SettingsDto; update: <K extends keyof SettingsDto>(key: K, value: SettingsDto[K]) => void; t: (key: string, fallback: string) => string }) {
  return <div className="grid option-grid">
    <label>{t("gui.label.audio_mode", "Audio mode")}<select value={settings.audioMode} onChange={(event) => update("audioMode", event.target.value)}><option value="copy">copy</option><option value="aac">aac</option></select></label>
    <label>{t("gui.label.audio_bitrate", "Audio bitrate")}<input disabled={settings.audioMode === "copy"} value={settings.audioBitrate} onChange={(event) => update("audioBitrate", event.target.value)} /></label>
    <label className="check"><input type="checkbox" checked={settings.copySubtitles} onChange={(event) => update("copySubtitles", event.target.checked)} />{t("gui.label.copy_subtitles", "Copy subtitles")}</label>
    <label className="check"><input type="checkbox" checked={settings.copyExternalSubtitles} onChange={(event) => update("copyExternalSubtitles", event.target.checked)} />{t("gui.label.copy_external_subtitles", "Copy external subtitles")}</label>
  </div>;
}

function PreviewOptions({ options, setOptions, onPreview, disabled, t }: { options: PreviewOptionsDto; setOptions: (value: PreviewOptionsDto) => void; onPreview: () => void; disabled: boolean; t: (key: string, fallback: string) => string }) {
  return <div className="grid option-grid">
    <label>{t("gui.label.sample_mode", "Sample mode")}<select value={options.sampleMode} onChange={(event) => setOptions({ ...options, sampleMode: event.target.value })}><option value="middle">middle</option><option value="custom">custom</option></select></label>
    <label>{t("gui.label.sample_duration", "Sample duration (s)")}<input type="number" min="1" value={options.sampleDurationSec} onChange={(event) => setOptions({ ...options, sampleDurationSec: Number(event.target.value) })} /></label>
    <label>{t("gui.label.sample_start", "Sample start (s)")}<input type="number" min="0" value={options.customStartSec ?? ""} onChange={(event) => setOptions({ ...options, customStartSec: event.target.value ? Number(event.target.value) : null })} /></label>
    <button disabled={disabled} onClick={onPreview}>{t("gui.button.preview", "Preview")}</button>
  </div>;
}

function AdvancedOptions({ appSettings, settings, update, updateApp, onSave, onPickWorkdir, onPickFfmpeg, onPickFfprobe, onRedetect, t }: { appSettings: AppSettingsDto; settings: SettingsDto; update: <K extends keyof SettingsDto>(key: K, value: SettingsDto[K]) => void; updateApp: <K extends keyof AppSettingsDto>(key: K, value: AppSettingsDto[K]) => void; onSave: () => void; onPickWorkdir: () => void; onPickFfmpeg: () => void; onPickFfprobe: () => void; onRedetect: () => void; t: (key: string, fallback: string) => string }) {
  return <div className="grid option-grid">
    <label>{t("gui.label.workdir", "Workdir")}<span className="field-row"><input value={appSettings.workdirPath} onChange={(event) => updateApp("workdirPath", event.target.value)} /><button type="button" onClick={onPickWorkdir}>{t("gui.button.browse_dir", "Pick Dir")}</button></span></label>
    <label>{t("gui.label.ffmpeg", "FFmpeg path")}<span className="field-row"><input value={appSettings.ffmpegPath} onChange={(event) => updateApp("ffmpegPath", event.target.value)} /><button type="button" onClick={onPickFfmpeg}>{t("gui.button.browse_exe", "Pick File")}</button></span></label>
    <label>{t("gui.label.ffprobe", "FFprobe path")}<span className="field-row"><input value={appSettings.ffprobePath} onChange={(event) => updateApp("ffprobePath", event.target.value)} /><button type="button" onClick={onPickFfprobe}>{t("gui.button.browse_exe", "Pick File")}</button></span></label>
    <label>{t("gui.label.log_level", "Log level")}<select value={appSettings.logLevel} onChange={(event) => updateApp("logLevel", event.target.value)}><option value="info">info</option><option value="debug">debug</option><option value="warn">warn</option></select></label>
    <label className="check"><input type="checkbox" checked={appSettings.keepPreviewTemp} onChange={(event) => updateApp("keepPreviewTemp", event.target.checked)} />{t("gui.label.keep_preview_temp", "Keep preview temp")}</label>
    <label>{t("gui.label.audio_bitrate", "Audio bitrate")}<input value={settings.audioBitrate} onChange={(event) => update("audioBitrate", event.target.value)} /></label>
    <button onClick={onRedetect}>{t("gui.button.redetect_encoders", "Re-detect encoders")}</button>
    <button onClick={onSave}>{t("gui.button.save_settings", "Save settings")}</button>
  </div>;
}

function Summary({ label, value }: { label: string; value: string }) { return <div><span>{label}</span><strong>{value}</strong></div>; }

function QueueTable({ items, selected, onSelect, t }: { items: QueueItemDto[]; selected: string[]; onSelect: (id: string, checked: boolean) => void; t: (key: string, fallback: string) => string }) {
  const headers: [string, string][] = [["gui.table.name", "Name"], ["gui.table.folder", "Folder"], ["gui.table.resolution", "Resolution"], ["gui.table.duration", "Duration"], ["gui.table.source_bitrate", "Source bitrate"], ["gui.table.target_bitrate", "Target bitrate"], ["gui.table.encoder", "Encoder"], ["gui.table.output", "Output"], ["gui.table.tags", "Tags"], ["gui.table.status", "Status"], ["gui.table.progress", "Progress"]];
  return <div className="table-wrap"><table><thead><tr><th aria-label="select" />{headers.map(([key, fallback]) => <th key={key}>{t(key, fallback)}</th>)}</tr></thead><tbody>{items.map((item) => <tr key={item.itemId}><td><input type="checkbox" checked={selected.includes(item.itemId)} onChange={(event) => onSelect(item.itemId, event.target.checked)} /></td><td>{item.plan.sourcePath.split(/[\\/]/).pop()}</td><td title={item.plan.sourcePath}>{item.plan.sourcePath.split(/[\\/]/).slice(0, -1).join("/") || "-"}</td><td>{item.plan.width && item.plan.height ? `${item.plan.width}x${item.plan.height}` : "n/a"}</td><td>{formatDuration(item.plan.duration)}</td><td>{item.plan.sourceBitrate ? `${Math.round(item.plan.sourceBitrate / 1000)} kbps` : "n/a"}</td><td>{item.plan.targetBitrate ? `${Math.round(item.plan.targetBitrate / 1000)} kbps` : "n/a"}</td><td>{item.plan.encoder ? `${item.plan.encoder} (${item.plan.backend})` : "n/a"}</td><td title={item.plan.outputPath}>{item.plan.outputPath.split(/[\\/]/).pop()}</td><td>{item.plan.warnings.length ? "Warn" : ""}</td><td>{item.status}</td><td>{item.status === "queued" ? "-" : `${item.progress.percent.toFixed(1)}%`}</td></tr>)}</tbody></table>{items.length === 0 && <div className="empty-table">{t("gui.message.queue_empty", "No jobs have been planned yet.")}</div>}</div>;
}

function AuxiliaryWindow({ kind }: { kind: AuxiliaryKind }) {
  const [bootstrap, setBootstrap] = useState<BootstrapDto | null>(null);
  const [snapshot, setSnapshot] = useRuntimeSnapshot(null);
  const [language, setLanguage] = useState<Language>("en");
  const [message, setMessage] = useState("");
  useEffect(() => { api.bootstrap().then((value) => { setBootstrap(value); setSnapshot(value.queue); setLanguage(value.appSettings.language === "zh_cn" ? "zh_cn" : "en"); }).catch((error) => setMessage(errorText(error))); }, [setSnapshot]);
  const t = (key: string, fallback: string) => translate(language, key, fallback);
  const title: Record<AuxiliaryKind, string> = { queue: t("gui.window.queue", "Queue"), activity: t("gui.window.activity", "Activity Log"), presets: t("gui.window.presets", "Preset Manager"), settings: t("gui.window.settings", "Settings"), preview: t("gui.window.preview", "Preview Result") };
  return <main className="aux-window"><div className="dialog-header"><h1>{title[kind]}</h1><button onClick={() => api.closeAuxiliary()}>×</button></div>{message && <p role="alert">{message}</p>}{kind === "queue" && <QueuePanel snapshot={snapshot} t={t} />}{kind === "activity" && <ActivityPanel t={t} />}{kind === "presets" && <PresetPanel initial={bootstrap?.settings ?? defaultSettings} appSettings={bootstrap?.appSettings ?? defaultAppSettings} t={t} />}{kind === "settings" && <SettingsPanel initial={bootstrap?.appSettings ?? defaultAppSettings} t={t} />}{kind === "preview" && <PreviewPanel t={t} />}</main>;
}

function QueuePanel({ snapshot, t }: { snapshot: QueueSnapshotDto; t: (key: string, fallback: string) => string }) {
  const [selected, setSelected] = useState<string[]>([]);
  const [message, setMessage] = useState("");
  const updateSelection = (id: string, checked: boolean) => setSelected((current) => checked ? [...current, id] : current.filter((value) => value !== id));
  async function run(action: () => Promise<unknown>) {
    try {
      await action();
    } catch (error) {
      setMessage(errorText(error));
    }
  }
  const move = async (delta: -1 | 1) => {
    const ids = snapshot.state.items.map((item) => item.itemId);
    if (selected.length !== 1) return;
    const index = ids.indexOf(selected[0]); const next = index + delta;
    if (index < 0 || next < 0 || next >= ids.length) return;
    [ids[index], ids[next]] = [ids[next], ids[index]];
    await run(() => api.reorderQueue(ids));
  };
  return <><p className="panel-summary">{t("gui.summary.states", "States")}: {snapshot.metrics.queuedItems} queued / {snapshot.metrics.runningItems} running / {snapshot.metrics.failedItems} failed</p><p className="queue-progress">{snapshot.metrics.queuePercent.toFixed(1)}% ({snapshot.metrics.completedItems}/{snapshot.metrics.totalItems}) · ETA {formatDuration(snapshot.metrics.etaSec)}</p><progress max="100" value={snapshot.metrics.queuePercent} />{message && <p role="alert">{message}</p>}<div className="inline-actions"><button disabled={selected.length === 0} onClick={() => run(() => api.queueRetry(selected))}>{t("gui.button.retry", "Retry")}</button><button disabled={selected.length === 0} onClick={() => run(() => api.removeQueue(selected))}>{t("gui.button.remove", "Remove")}</button><button onClick={() => move(-1)}>↑</button><button onClick={() => move(1)}>↓</button><button onClick={() => run(() => api.clearCompleted())}>{t("gui.button.clear_completed", "Clear completed")}</button></div><QueueTable items={snapshot.state.items} selected={selected} onSelect={updateSelection} t={t} /></>;
}

function ActivityPanel({ t }: { t: (key: string, fallback: string) => string }) {
  const [events, setEvents] = useState<ActivityEventDto[]>([]);
  const [filter, setFilter] = useState("all");
  const [message, setMessage] = useState("");
  useEffect(() => { api.activityHistory().then(setEvents).catch(() => undefined); api.subscribeActivity((message) => { if (message.type === "activity") setEvents((current) => [...current, message.data].slice(-500)); }).catch(() => undefined); }, []);
  const visible = filter === "all" ? events : events.filter((event) => event.category === filter);
  async function clear() {
    try {
      await api.activityClear();
      setEvents([]);
    } catch (error) {
      setMessage(errorText(error));
    }
  }
  async function exportLog() {
    try {
      await api.exportActivity();
      setMessage(t("gui.message.activity_exported", "Activity log exported."));
    } catch (error) {
      setMessage(errorText(error));
    }
  }
  return <><div className="activity-controls"><label>{t("gui.label.log_filter", "Log filter")}<select value={filter} onChange={(event) => setFilter(event.target.value)}><option value="all">{t("gui.filter.all", "All")}</option><option value="command">{t("gui.filter.command", "Command")}</option><option value="process">{t("gui.filter.process", "Process")}</option><option value="error">{t("gui.filter.error", "Error")}</option></select></label><div className="inline-actions"><button onClick={exportLog}>{t("gui.button.export_log", "Export Log")}</button><button onClick={clear}>{t("gui.button.clear_log", "Clear Log")}</button></div></div>{message && <p role="alert">{message}</p>}<div className="activity-list">{visible.length === 0 ? <p>{t("gui.message.activity_empty", "No activity yet.")}</p> : visible.map((event, index) => <div className="activity-row" key={`${event.timestamp}-${index}`}><time>{event.timestamp}</time><strong>{event.category}</strong><span>{event.message}</span></div>)}</div></>;
}

function PresetPanel({ initial, appSettings, t }: { initial: SettingsDto; appSettings: AppSettingsDto; t: (key: string, fallback: string) => string }) {
  const [names, setNames] = useState<string[]>([]); const [selected, setSelected] = useState(""); const [name, setName] = useState(""); const [settings, setSettings] = useState(initial); const [message, setMessage] = useState("");
  useEffect(() => setSettings(initial), [initial]);
  useEffect(() => { api.listPresets().then(setNames).catch((error) => setMessage(errorText(error))); }, []);
  async function load(value: string) { setSelected(value); if (value) try { setSettings(await api.loadPreset(value)); } catch (error) { setMessage(errorText(error)); } }
  async function save() { if (!name.trim()) return; try { await api.savePreset(name.trim(), settings); setNames(await api.listPresets()); setSelected(name.trim()); setMessage(t("gui.message.preset_saved", "Preset saved.")); } catch (error) { setMessage(errorText(error)); } }
  async function remove() { if (!selected) return; try { await api.deletePreset(selected); setNames(await api.listPresets()); setSelected(""); } catch (error) { setMessage(errorText(error)); } }
  async function setDefault() { if (!selected) return; try { await api.saveAppSettings({ ...appSettings, defaultPresetName: selected }); setMessage(t("gui.button.set_default_preset", "Set Default")); } catch (error) { setMessage(errorText(error)); } }
  return <section className="panel-form"><label>{t("gui.label.preset", "Preset")}<select value={selected} onChange={(event) => load(event.target.value)}><option value="">{t("gui.value.select", "Select a preset")}</option>{names.map((value) => <option key={value} value={value}>{value}{value === appSettings.defaultPresetName ? " ✓" : ""}</option>)}</select></label><label>{t("gui.label.name", "Name")}<input value={name} onChange={(event) => setName(event.target.value)} /></label><div className="inline-actions"><button disabled={!selected} onClick={() => load(selected)}>{t("gui.button.load_preset", "Load")}</button><button onClick={save}>{t("gui.button.save_preset", "Save")}</button><button disabled={!selected} onClick={setDefault}>{t("gui.button.set_default_preset", "Set Default")}</button><button disabled={!selected} onClick={remove}>{t("gui.button.delete_preset", "Delete")}</button></div>{message && <p role="alert">{message}</p>}<pre className="json-preview">{JSON.stringify(settings, null, 2)}</pre></section>;
}

function SettingsPanel({ initial, t }: { initial: AppSettingsDto; t: (key: string, fallback: string) => string }) {
  const [settings, setSettings] = useState(initial); const update = <K extends keyof AppSettingsDto>(key: K, value: AppSettingsDto[K]) => setSettings((current) => ({ ...current, [key]: value })); const [message, setMessage] = useState("");
  useEffect(() => setSettings(initial), [initial]);
  async function save() { try { await api.saveAppSettings(settings); setMessage(t("gui.message.settings_saved", "Settings saved.")); } catch (error) { setMessage(errorText(error)); } }
  async function pick(key: "workdirPath" | "ffmpegPath" | "ffprobePath", directory: boolean) { try { const value = directory ? await api.pickDirectory() : await api.pickFile(); if (typeof value === "string") update(key, value); } catch (error) { setMessage(errorText(error)); } }
  async function redetect() { try { await api.saveAppSettings(settings); await api.redetectEncoders(); setMessage(t("gui.button.redetect_encoders", "Re-detect encoders")); } catch (error) { setMessage(errorText(error)); } }
  return <section className="panel-form"><label>{t("gui.label.language", "Language")}<select value={settings.language} onChange={(event) => update("language", event.target.value)}><option value="en">English</option><option value="zh_cn">简体中文</option></select></label><label>{t("gui.label.workdir", "Workdir")}<span className="field-row"><input value={settings.workdirPath} onChange={(event) => update("workdirPath", event.target.value)} /><button type="button" onClick={() => pick("workdirPath", true)}>{t("gui.button.browse_dir", "Pick Dir")}</button></span></label><label>{t("gui.label.ffmpeg", "FFmpeg path")}<span className="field-row"><input value={settings.ffmpegPath} onChange={(event) => update("ffmpegPath", event.target.value)} /><button type="button" onClick={() => pick("ffmpegPath", false)}>{t("gui.button.browse_exe", "Pick File")}</button></span></label><label>{t("gui.label.ffprobe", "FFprobe path")}<span className="field-row"><input value={settings.ffprobePath} onChange={(event) => update("ffprobePath", event.target.value)} /><button type="button" onClick={() => pick("ffprobePath", false)}>{t("gui.button.browse_exe", "Pick File")}</button></span></label><label>{t("gui.label.log_level", "Log level")}<select value={settings.logLevel} onChange={(event) => update("logLevel", event.target.value)}><option>info</option><option>debug</option><option>warn</option></select></label><label className="check"><input type="checkbox" checked={settings.keepPreviewTemp} onChange={(event) => update("keepPreviewTemp", event.target.checked)} />{t("gui.label.keep_preview_temp", "Keep preview temp")}</label><div className="inline-actions"><button onClick={redetect}>{t("gui.button.redetect_encoders", "Re-detect encoders")}</button><button onClick={save}>{t("gui.button.save_settings", "Save settings")}</button></div>{message && <p role="alert">{message}</p>}</section>;
}

function PreviewPanel({ t }: { t: (key: string, fallback: string) => string }) {
  const [result, setResult] = useState<PreviewResultDto | null>(null);
  useEffect(() => {
    try {
      const raw = localStorage.getItem("video-compressor.preview-result");
      if (raw) setResult(JSON.parse(raw) as PreviewResultDto);
    } catch {
      setResult(null);
    }
  }, []);
  if (!result) {
    return <section className="panel-form"><p>{t("gui.message.preview_empty", "Run Preview from the main window to inspect a sample.")}</p></section>;
  }
  return <section className="panel-form preview-result"><div><span>{t("gui.label.source", "Source")}</span><code>{result.sourcePath}</code></div><div><span>{t("gui.label.sample_source", "Source sample")}</span><code>{result.sourceSamplePath}</code></div><div><span>{t("gui.label.sample_encoded", "Encoded sample")}</span><code>{result.encodedSamplePath}</code></div><div className="summary-grid"><Summary label={t("gui.label.sample_source_size", "Source sample size")} value={formatBytes(result.sampleSourceSize)} /><Summary label={t("gui.label.sample_encoded_size", "Encoded sample size")} value={formatBytes(result.sampleEncodedSize)} /><Summary label={t("gui.label.sample_ratio", "Sample ratio")} value={result.sampleCompressionRatio.toFixed(3)} /><Summary label={t("gui.label.estimated_output", "Estimated output")} value={formatBytes(result.estimatedFullOutputSize)} /></div>{result.notes.map((note) => <p key={note}>{note}</p>)}{result.logPath && <code>{result.logPath}</code>}</section>;
}

export default App;
