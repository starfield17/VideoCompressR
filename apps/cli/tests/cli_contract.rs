use std::path::PathBuf;
use std::process::Command;
#[cfg(unix)]
use std::time::Duration;

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
    let missing = temp.path().join("definitely-missing.mp4");
    let missing_value = missing.to_string_lossy().into_owned();
    let output = run_cli(temp.path(), &["plan", &missing_value]);
    assert_eq!(output.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Cannot access input path"));
}

#[test]
fn valid_input_with_missing_tools_uses_tool_exit_code() {
    let temp = tempfile::tempdir().expect("temp");
    let source = temp.path().join("video.mp4");
    std::fs::write(&source, b"fixture").expect("source");
    let ffmpeg = temp.path().join("missing-ffmpeg");
    let ffprobe = temp.path().join("missing-ffprobe");
    let source_value = source.to_string_lossy().into_owned();
    let ffmpeg_value = ffmpeg.to_string_lossy().into_owned();
    let ffprobe_value = ffprobe.to_string_lossy().into_owned();
    let output = run_cli(
        temp.path(),
        &["plan", &source_value, "--ffmpeg", &ffmpeg_value, "--ffprobe", &ffprobe_value],
    );
    assert_eq!(output.status.code(), Some(3));
    assert!(!output.stderr.is_empty());
}

#[test]
fn unsupported_input_extension_uses_planning_exit_code() {
    let temp = tempfile::tempdir().expect("temp");
    let source = temp.path().join("notes.txt");
    std::fs::write(&source, b"fixture").expect("source");
    let source_value = source.to_string_lossy().into_owned();
    let output = run_cli(temp.path(), &["plan", &source_value]);
    assert_eq!(output.status.code(), Some(4));
}

#[test]
fn empty_input_directory_uses_planning_exit_code() {
    let temp = tempfile::tempdir().expect("temp");
    let input = temp.path().join("empty");
    std::fs::create_dir(&input).expect("input directory");
    let input_value = input.to_string_lossy().into_owned();
    let output = run_cli(temp.path(), &["plan", &input_value]);
    assert_eq!(output.status.code(), Some(4));
}

#[test]
fn invalid_preset_uses_configuration_exit_code() {
    let temp = tempfile::tempdir().expect("temp");
    let output = run_cli(temp.path(), &["preset", "load", "missing-preset"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn probe_failure_uses_planning_exit_code() {
    let temp = tempfile::tempdir().expect("temp");
    let source = temp.path().join("probe-failure.mp4");
    std::fs::write(&source, b"fixture").expect("source");
    let ffmpeg = fake("fake-ffmpeg").to_string_lossy().into_owned();
    let ffprobe = fake("fake-ffprobe-fail").to_string_lossy().into_owned();
    let source_value = source.to_string_lossy().into_owned();
    let output = run_cli(
        temp.path(),
        &["plan", &source_value, "--backend", "cpu", "--ffmpeg", &ffmpeg, "--ffprobe", &ffprobe],
    );
    assert_eq!(output.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&output.stderr).contains("ffprobe fixture failure"));
}

#[test]
fn encode_failure_uses_encode_exit_code() {
    let temp = tempfile::tempdir().expect("temp");
    let source = temp.path().join("fail-item.mp4");
    std::fs::write(&source, b"fixture").expect("source");
    let ffmpeg = fake("fake-ffmpeg-item-fail").to_string_lossy().into_owned();
    let ffprobe = fake("fake-ffprobe").to_string_lossy().into_owned();
    let source_value = source.to_string_lossy().into_owned();
    let output = run_cli(
        temp.path(),
        &[
            "encode",
            &source_value,
            "--backend",
            "cpu",
            "--overwrite",
            "--ffmpeg",
            &ffmpeg,
            "--ffprobe",
            &ffprobe,
        ],
    );
    assert_eq!(output.status.code(), Some(5));
}

#[cfg(unix)]
#[test]
fn cancellation_uses_exit_code_130() {
    let temp = tempfile::tempdir().expect("temp");
    let source = temp.path().join("cancel-me.mp4");
    std::fs::write(&source, b"fixture").expect("source");
    let ffmpeg = fake("fake-queue-hang").to_string_lossy().into_owned();
    let ffprobe = fake("fake-ffprobe").to_string_lossy().into_owned();
    let source_value = source.to_string_lossy().into_owned();
    let child = Command::new(env!("CARGO_BIN_EXE_video-compressor"))
        .env("VIDEO_COMPRESSOR_DATA_DIR", temp.path())
        .args([
            "encode",
            &source_value,
            "--backend",
            "cpu",
            "--overwrite",
            "--ffmpeg",
            &ffmpeg,
            "--ffprobe",
            &ffprobe,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("CLI process");
    std::thread::sleep(Duration::from_secs(2));
    let signal =
        Command::new("kill").args(["-INT", &child.id().to_string()]).status().expect("send SIGINT");
    assert!(signal.success());
    let output = child.wait_with_output().expect("CLI output");
    assert_eq!(output.status.code(), Some(130));
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
