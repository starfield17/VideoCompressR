use std::path::PathBuf;
use std::process::Command;

fn fake(name: &str) -> PathBuf {
    let filename = if cfg!(windows) { format!("{name}.ps1") } else { format!("{name}.sh") };
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/fake-tools").join(filename)
}

fn run_cli(data_dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_video-compressor"))
        .env("VIDEO_COMPRESSOR_DATA_DIR", data_dir)
        .args(args)
        .output()
        .expect("CLI process")
}

#[test]
fn plan_stdout_stderr_exit_code_and_preset_contract_are_stable() {
    let temp = tempfile::tempdir().expect("temp");
    let source = temp.path().join("movie with space.mp4");
    std::fs::write(&source, b"fixture").expect("source");
    let ffmpeg = fake("fake-ffmpeg").to_string_lossy().into_owned();
    let ffprobe = fake("fake-ffprobe").to_string_lossy().into_owned();
    let source_value = source.to_string_lossy().into_owned();
    let output = run_cli(
        temp.path(),
        &[
            "plan",
            &source_value,
            "--backend",
            "cpu",
            "--overwrite",
            "--ffmpeg",
            &ffmpeg,
            "--ffprobe",
            &ffprobe,
            "--lang",
            "en",
        ],
    );
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Plan items: 1"));
    assert!(stdout.contains("movie with space.mp4"));
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let save = run_cli(temp.path(), &["preset", "save", "ci-av1", "--codec", "av1"]);
    assert!(save.status.success(), "stderr: {}", String::from_utf8_lossy(&save.stderr));
    let load = run_cli(temp.path(), &["preset", "load", "ci-av1"]);
    assert!(load.status.success(), "stderr: {}", String::from_utf8_lossy(&load.stderr));
    assert!(String::from_utf8_lossy(&load.stdout).contains("\"codec\": \"av1\""));
}

#[test]
fn invalid_input_uses_planning_exit_code() {
    let temp = tempfile::tempdir().expect("temp");
    let output = run_cli(temp.path(), &["plan", "missing.mp4"]);
    assert_eq!(output.status.code(), Some(4));
    assert!(!output.stderr.is_empty());
}

#[test]
fn encode_dry_run_prints_the_plan_without_creating_output() {
    let temp = tempfile::tempdir().expect("temp");
    let source = temp.path().join("dry-run.mp4");
    std::fs::write(&source, b"fixture").expect("source");
    let output_dir = temp.path().join("output");
    let ffmpeg = fake("fake-ffmpeg").to_string_lossy().into_owned();
    let ffprobe = fake("fake-ffprobe").to_string_lossy().into_owned();
    let source_value = source.to_string_lossy().into_owned();
    let output_value = output_dir.to_string_lossy().into_owned();
    let output = run_cli(
        temp.path(),
        &[
            "encode",
            &source_value,
            "--output",
            &output_value,
            "--backend",
            "cpu",
            "--overwrite",
            "--dry-run",
            "--ffmpeg",
            &ffmpeg,
            "--ffprobe",
            &ffprobe,
        ],
    );
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("Plan items: 1"));
    assert!(!output_dir.join("dry-run_hevc.mp4").exists());
}
