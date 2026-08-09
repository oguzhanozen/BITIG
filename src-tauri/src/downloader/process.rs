use std::{ffi::OsString, path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader},
    process::Command,
    sync::{mpsc, watch},
    time::sleep,
};

use crate::errors::{AppError, AppResult};

pub struct ProcessSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub timeout: Duration,
    pub stdout_limit: u64,
    pub stderr_limit: u64,
}

#[derive(Debug)]
pub struct ProcessOutput {
    pub success: bool,
    pub stdout: Vec<u8>,
}

#[derive(Clone, Default)]
pub struct CancellationToken {
    sender: Arc<watch::Sender<bool>>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        self.sender.send_replace(true);
    }

    async fn cancelled(&self) {
        let mut receiver = self.sender.subscribe();
        while !*receiver.borrow_and_update() {
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct BoundedProcessRunner;

impl BoundedProcessRunner {
    pub async fn run(&self, spec: ProcessSpec) -> AppResult<ProcessOutput> {
        self.run_cancellable(spec, CancellationToken::default())
            .await
    }

    pub async fn run_cancellable(
        &self,
        spec: ProcessSpec,
        cancellation: CancellationToken,
    ) -> AppResult<ProcessOutput> {
        self.run_internal(spec, cancellation, None).await
    }

    pub async fn run_cancellable_streaming(
        &self,
        spec: ProcessSpec,
        cancellation: CancellationToken,
        lines: mpsc::UnboundedSender<String>,
    ) -> AppResult<ProcessOutput> {
        self.run_internal(spec, cancellation, Some(lines)).await
    }

    async fn run_internal(
        &self,
        spec: ProcessSpec,
        cancellation: CancellationToken,
        lines: Option<mpsc::UnboundedSender<String>>,
    ) -> AppResult<ProcessOutput> {
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        #[cfg(windows)]
        command.creation_flags(0x08000000);

        let mut child = command.spawn().map_err(|error| {
            tracing::error!(program = %spec.program.display(), %error, "external tool failed to start");
            if error.kind() == std::io::ErrorKind::NotFound {
                AppError::ToolUnavailable
            } else {
                AppError::Analysis(error.to_string())
            }
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppError::Analysis("stdout was unavailable".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AppError::Analysis("stderr was unavailable".into()))?;

        enum Outcome {
            Completed(AppResult<ProcessOutput>),
            TimedOut,
            Cancelled,
        }

        let outcome = {
            let operation = async {
                let (stdout, _, status) = tokio::try_join!(
                    read_limited(stdout, spec.stdout_limit, lines.clone()),
                    read_limited(stderr, spec.stderr_limit, lines),
                    async { child.wait().await.map_err(AppError::from) }
                )?;
                Ok::<_, AppError>(ProcessOutput {
                    success: status.success(),
                    stdout,
                })
            };
            tokio::pin!(operation);
            tokio::select! {
                result = &mut operation => Outcome::Completed(result),
                _ = sleep(spec.timeout) => Outcome::TimedOut,
                _ = cancellation.cancelled() => Outcome::Cancelled,
            }
        };

        match outcome {
            Outcome::Completed(Ok(output)) => Ok(output),
            Outcome::Completed(Err(error)) => {
                let _ = child.kill().await;
                Err(error)
            }
            Outcome::TimedOut => {
                let _ = child.kill().await;
                tracing::warn!(program = %spec.program.display(), "external tool timed out");
                Err(AppError::ProcessTimeout)
            }
            Outcome::Cancelled => {
                let _ = child.kill().await;
                Err(AppError::Cancelled)
            }
        }
    }
}

async fn read_limited(
    reader: impl AsyncRead + Unpin,
    limit: u64,
    lines: Option<mpsc::UnboundedSender<String>>,
) -> AppResult<Vec<u8>> {
    let mut reader = BufReader::new(reader.take(limit + 1));
    let mut output = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line).await?;
        if read == 0 {
            break;
        }
        output.extend_from_slice(&line);
        if output.len() as u64 > limit {
            return Err(AppError::ProcessOutputTooLarge);
        }
        if let Some(sender) = &lines {
            let value = String::from_utf8_lossy(&line)
                .trim_end_matches(['\r', '\n'])
                .to_owned();
            if !value.is_empty() {
                let _ = sender.send(value);
            }
        }
    }
    Ok(output)
}

#[cfg(all(test, windows))]
mod tests {
    use super::{BoundedProcessRunner, CancellationToken, ProcessSpec};
    use crate::errors::AppError;
    use std::{ffi::OsString, path::PathBuf, time::Duration};

    fn slow_process(timeout: Duration) -> ProcessSpec {
        ProcessSpec {
            program: PathBuf::from("ping.exe"),
            args: ["-n", "6", "127.0.0.1"]
                .into_iter()
                .map(OsString::from)
                .collect(),
            timeout,
            stdout_limit: 64 * 1024,
            stderr_limit: 64 * 1024,
        }
    }

    #[tokio::test]
    async fn process_timeout_terminates_the_child() {
        let error = BoundedProcessRunner
            .run(slow_process(Duration::from_millis(20)))
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::ProcessTimeout));
    }

    #[tokio::test]
    async fn cancellation_terminates_the_child() {
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        let error = BoundedProcessRunner
            .run_cancellable(slow_process(Duration::from_secs(10)), cancellation)
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::Cancelled));
    }
}
