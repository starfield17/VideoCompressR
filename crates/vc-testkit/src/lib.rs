//! Test support for real fake executables and temporary application layouts.

use std::path::PathBuf;
use tempfile::TempDir;
use vc_runtime::AppPaths;

pub fn fake_tool(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/fake-tools").join(name)
}

pub fn temp_app() -> (TempDir, AppPaths) {
    let directory = tempfile::tempdir().expect("temporary app directory");
    let paths = AppPaths::from_root(directory.path());
    paths.ensure().expect("temporary app layout");
    (directory, paths)
}
