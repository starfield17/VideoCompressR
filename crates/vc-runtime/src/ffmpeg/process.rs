use crate::error::RuntimeError;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
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
    let mut stdout = String::new();
    let mut stderr = String::new();
    let output = run_streaming(request, cancel, |line| {
        let target = match line.stream {
            OutputStream::Stdout => &mut stdout,
            OutputStream::Stderr => &mut stderr,
        };
        target.push_str(&line.text);
        target.push('\n');
    })
    .await?;
    if output.cancelled {
        return Err(RuntimeError::Cancelled);
    }
    Ok((output.code, stdout, stderr))
}

pub async fn run_streaming<F>(
    request: ToolRequest,
    cancel: CancellationToken,
    mut on_line: F,
) -> Result<ProcessOutput, RuntimeError>
where
    F: FnMut(ProcessLine) + Send,
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
    let (tx, mut rx) = mpsc::unbounded_channel::<ProcessLine>();
    let stdout_tx = tx.clone();
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = stdout_tx.send(ProcessLine { stream: OutputStream::Stdout, text: line });
        }
    });
    let stderr_tx = tx.clone();
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = stderr_tx.send(ProcessLine { stream: OutputStream::Stderr, text: line });
        }
    });
    drop(tx);

    let child_cancel = cancel.clone();
    let mut wait_task = Box::pin(tokio::spawn(async move {
        let mut child = child;
        tokio::select! {
            result = child.wait() => Ok::<(std::process::ExitStatus, bool), std::io::Error>((result?, false)),
            _ = child_cancel.cancelled() => {
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

    let mut completion: Option<(std::process::ExitStatus, bool)> = None;
    loop {
        if let Some((status, cancelled)) = completion.take() {
            while let Some(line) = rx.recv().await {
                on_line(line);
            }
            let _ = stdout_task.await;
            let _ = stderr_task.await;
            let code = status.code().unwrap_or(-1);
            return Ok(ProcessOutput { code, cancelled });
        }
        tokio::select! {
            line = rx.recv() => match line { Some(value) => on_line(value), None => { completion = Some(wait_task.as_mut().await.map_err(|error| RuntimeError::Encode(error.to_string()))??); } },
            result = &mut wait_task => { completion = Some(result.map_err(|error| RuntimeError::Encode(error.to_string()))??); },
        }
    }
}
