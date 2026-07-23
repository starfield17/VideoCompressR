use crate::error::RuntimeError;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex, mpsc};
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;

#[cfg(windows)]
use std::os::windows::io::{FromRawHandle, OwnedHandle};

#[cfg(windows)]
use windows_sys::Win32::Foundation::HANDLE;
#[cfg(windows)]
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};

#[cfg(windows)]
#[allow(dead_code)]
struct ProcessJob(OwnedHandle);

#[cfg(windows)]
impl ProcessJob {
    fn assign(child: &tokio::process::Child) -> Result<Self, RuntimeError> {
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(RuntimeError::Encode(std::io::Error::last_os_error().to_string()));
        }
        let owned = unsafe { OwnedHandle::from_raw_handle(handle) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let info_ok = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if info_ok == 0 {
            return Err(RuntimeError::Encode(std::io::Error::last_os_error().to_string()));
        }
        let process = child.raw_handle().ok_or_else(|| {
            RuntimeError::Encode(
                "FFmpeg process exited before it could join its Job Object.".into(),
            )
        })?;
        let assigned = unsafe { AssignProcessToJobObject(handle, process as HANDLE) };
        if assigned == 0 {
            return Err(RuntimeError::Encode(std::io::Error::last_os_error().to_string()));
        }
        Ok(Self(owned))
    }
}

#[derive(Clone, Debug)]
pub struct ToolRequest {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug)]
pub struct ProcessLine {
    pub stream: OutputStream,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct ProcessOutput {
    pub code: i32,
    pub cancelled: bool,
}

#[derive(Clone, Debug)]
pub struct CapturedOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
    pub cancelled: bool,
}

fn command_for(request: &ToolRequest) -> Command {
    #[cfg(windows)]
    let mut command = if request.program.extension().and_then(|value| value.to_str()) == Some("ps1")
    {
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
        command.arg(&request.program);
        command
    } else {
        Command::new(&request.program)
    };
    #[cfg(not(windows))]
    let mut command = Command::new(&request.program);
    command.args(&request.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(cwd) = &request.cwd {
        command.current_dir(cwd);
    }
    #[cfg(unix)]
    {
        // A process group lets cancellation reach FFmpeg children without a shell.
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
            });
        }
    }
    command.kill_on_drop(true);
    command
}

pub async fn run_capture(
    request: ToolRequest,
    cancel: CancellationToken,
) -> Result<(i32, String, String), RuntimeError> {
    let output = run_capture_exact(request, cancel).await?;
    if output.cancelled {
        return Err(RuntimeError::Cancelled);
    }
    Ok((output.code, output.stdout, output.stderr))
}

pub async fn run_capture_exact(
    request: ToolRequest,
    cancel: CancellationToken,
) -> Result<CapturedOutput, RuntimeError> {
    let stdout = std::sync::Arc::new(Mutex::new(String::new()));
    let stderr = std::sync::Arc::new(Mutex::new(String::new()));
    let stdout_for_line = stdout.clone();
    let stderr_for_line = stderr.clone();
    let output = run_streaming(request, cancel, move |line| {
        let target = match line.stream {
            OutputStream::Stdout => stdout_for_line.clone(),
            OutputStream::Stderr => stderr_for_line.clone(),
        };
        async move {
            let mut target = target.lock().await;
            target.push_str(&line.text);
            target.push('\n');
            Ok(())
        }
    })
    .await?;
    Ok(CapturedOutput {
        code: output.code,
        stdout: stdout.lock().await.clone(),
        stderr: stderr.lock().await.clone(),
        cancelled: output.cancelled,
    })
}

pub async fn run_streaming<F, Fut>(
    request: ToolRequest,
    cancel: CancellationToken,
    mut on_line: F,
) -> Result<ProcessOutput, RuntimeError>
where
    F: FnMut(ProcessLine) -> Fut + Send,
    Fut: std::future::Future<Output = Result<(), RuntimeError>> + Send,
{
    let mut child = command_for(&request).spawn()?;
    #[cfg(windows)]
    let job = ProcessJob::assign(&child)?;
    let stdin = child.stdin.take();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| RuntimeError::Encode("FFmpeg stdout pipe was unavailable".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| RuntimeError::Encode("FFmpeg stderr pipe was unavailable".into()))?;
    // Both streams await bounded-channel capacity. Coalescing belongs at the progress sink,
    // not at the process reader, because capture and diagnostics must remain lossless.
    let (tx, mut rx) = mpsc::channel::<ProcessLine>(1_024);
    let stdout_tx = tx.clone();
    let stdout_task = tokio::spawn(read_stream(stdout, OutputStream::Stdout, stdout_tx));
    let stderr_tx = tx.clone();
    let stderr_task = tokio::spawn(read_stream(stderr, OutputStream::Stderr, stderr_tx));
    drop(tx);

    let internal_cancel = CancellationToken::new();
    let child_cancel = cancel.clone();
    let internal_child_cancel = internal_cancel.clone();
    let mut wait_task = Box::pin(tokio::spawn(async move {
        let mut child = child;
        tokio::select! {
            result = child.wait() => Ok::<(std::process::ExitStatus, bool), std::io::Error>((result?, false)),
            _ = async {
                tokio::select! {
                    _ = child_cancel.cancelled() => {},
                    _ = internal_child_cancel.cancelled() => {},
                }
            } => {
                if let Some(mut stdin) = stdin { let _ = stdin.write_all(b"q\n").await; let _ = stdin.flush().await; }
                sleep(Duration::from_millis(350)).await;
                if child.try_wait()?.is_none() {
                    #[cfg(windows)]
                    drop(job);
                    #[cfg(unix)]
                    if let Some(pid) = child.id() { unsafe { libc::kill(-(pid as i32), libc::SIGTERM); } }
                    sleep(Duration::from_millis(350)).await;
                    #[cfg(unix)]
                    if let Some(pid) = child.id() {
                        if child.try_wait()?.is_none() { unsafe { libc::kill(-(pid as i32), libc::SIGKILL); } }
                    }
                    let _ = child.kill().await;
                }
                let status = child.wait().await?;
                Ok((status, true))
            }
        }
    }));

    let mut handler_error = None;
    let process_result: Result<(std::process::ExitStatus, bool), RuntimeError> = loop {
        if handler_error.is_some() {
            break wait_task
                .await
                .map_err(|error| RuntimeError::Encode(error.to_string()))?
                .map_err(RuntimeError::from);
        }
        tokio::select! {
            line = rx.recv() => match line {
                Some(value) => {
                    if let Err(error) = on_line(value).await {
                        handler_error = Some(error);
                        internal_cancel.cancel();
                    }
                }
                None => {
                    break wait_task
                        .await
                        .map_err(|error| RuntimeError::Encode(error.to_string()))?
                        .map_err(RuntimeError::from);
                }
            },
            result = &mut wait_task => {
                break result
                    .map_err(|error| RuntimeError::Encode(error.to_string()))?
                    .map_err(RuntimeError::from);
            },
        }
    };

    while let Some(line) = rx.recv().await {
        if handler_error.is_none() {
            if let Err(error) = on_line(line).await {
                handler_error = Some(error);
                internal_cancel.cancel();
            }
        }
    }
    let stdout_result =
        stdout_task.await.map_err(|error| RuntimeError::Encode(error.to_string()))?;
    let stderr_result =
        stderr_task.await.map_err(|error| RuntimeError::Encode(error.to_string()))?;
    if let Some(error) = handler_error {
        return Err(error);
    }
    stdout_result?;
    stderr_result?;
    let (status, cancelled) = process_result?;
    Ok(ProcessOutput { code: status.code().unwrap_or(-1), cancelled })
}

async fn read_stream<R>(
    reader: R,
    stream: OutputStream,
    sender: mpsc::Sender<ProcessLine>,
) -> Result<(), RuntimeError>
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        let count = reader.read_until(b'\n', &mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        while matches!(buffer.last(), Some(b'\n' | b'\r')) {
            buffer.pop();
        }
        let text = String::from_utf8_lossy(&buffer).into_owned();
        sender
            .send(ProcessLine { stream, text })
            .await
            .map_err(|_| RuntimeError::Encode("process output consumer closed".into()))?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::task::{Context, Poll};
    use tokio::io::ReadBuf;

    struct ErrorReader;

    impl AsyncRead for ErrorReader {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Ready(Err(std::io::Error::other("fake reader failure")))
        }
    }

    #[tokio::test]
    async fn reader_error_is_propagated() {
        let (sender, _receiver) = mpsc::channel(1);
        let error = read_stream(ErrorReader, OutputStream::Stdout, sender)
            .await
            .expect_err("reader error must be visible");
        assert!(error.to_string().contains("fake reader failure"));
    }
}
