use std::{
    cell::Cell,
    fs,
    future::Future,
    io,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use youtube_captions_dl::{Cache, Captions, Client, Runtime};

pub(super) struct Fixture {
    pub(super) dir: tempfile::TempDir,
    pub(super) runtime: Runtime,
    unfinished: Cell<usize>,
}

impl Fixture {
    pub(super) fn new(script: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("yt-dlp");
        fs::write(&executable, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let mut client = Client::new().unwrap();
        client.runner.path = Some(dir.path().as_os_str().to_owned());
        Self {
            runtime: Runtime {
                cache: Cache {
                    directory: dir.path().join("cache"),
                },
                client,
            },
            dir,
            unfinished: Cell::new(0),
        }
    }

    pub(super) fn metadata(metadata: serde_json::Value) -> Self {
        let json = metadata.to_string().replace('\'', "'\\''");
        Self::new(&format!("printf '%s\\n' '{json}'"))
    }

    /// Returning from the client awaits process cleanup; dropping its future does not.
    pub(super) async fn track<T>(&self, work: impl Future<Output = T>) -> T {
        self.unfinished.set(self.unfinished.get() + 1);
        let result = work.await;
        self.unfinished.set(self.unfinished.get() - 1);
        result
    }

    pub(super) async fn fetch(
        &self,
        video: &str,
        token: &CancellationToken,
    ) -> anyhow::Result<Captions> {
        self.track(self.runtime.client.fetch(video, token)).await
    }
}

/// Observes the stand-in group; its owning Fixture performs final shutdown.
pub(super) struct ProcessGroup(pub(super) PathBuf);

impl ProcessGroup {
    pub(super) fn leader(&self) -> io::Result<Option<rustix::process::Pid>> {
        let text = match fs::read_to_string(self.0.join("leader.pid")) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        text.trim()
            .parse()
            .ok()
            .filter(|pid| *pid > 0)
            .and_then(rustix::process::Pid::from_raw)
            .map(Some)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid fixture process ID"))
    }

    /// Waits until no member of the group remains, including unreaped ones.
    pub(super) fn wait_for_exit(&self) -> io::Result<()> {
        let leader = self
            .leader()
            .unwrap()
            .expect("the stand-in recorded its pid");
        Self::wait_for(leader, Duration::from_secs(5))
    }

    fn wait_for(leader: rustix::process::Pid, timeout: Duration) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            match rustix::process::test_kill_process_group(leader) {
                Err(rustix::io::Errno::SRCH) => return Ok(()),
                // macOS can report EPERM while only unreaped zombies remain.
                // It is not proof of exit: continue until ESRCH or the deadline.
                Err(rustix::io::Errno::PERM) if Instant::now() < deadline => {}
                Err(error) => return Err(error.into()),
                Ok(()) if Instant::now() >= deadline => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("process group {leader:?} still exists"),
                    ));
                }
                Ok(()) => {}
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn stop(&self, unfinished: usize) -> io::Result<()> {
        let leader = match self.leader()? {
            Some(leader) => leader,
            None if unfinished == 0 => return Ok(()),
            None => {
                return Err(io::Error::other(
                    "unfinished operation without a process ID",
                ));
            }
        };
        for (signal, grace) in [
            (rustix::process::Signal::TERM, Duration::from_millis(300)),
            (rustix::process::Signal::KILL, Duration::from_secs(5)),
        ] {
            match rustix::process::kill_process_group(leader, signal) {
                Err(rustix::io::Errno::SRCH) => return Ok(()),
                Ok(()) | Err(rustix::io::Errno::PERM) => {}
                Err(error) => return Err(error.into()),
            }
            match Self::wait_for(leader, grace) {
                Ok(()) => return Ok(()),
                Err(error)
                    if signal == rustix::process::Signal::TERM
                        && error.kind() == io::ErrorKind::TimedOut => {}
                Err(error) => return Err(error),
            }
        }
        unreachable!("the final wait either confirms exit or reports its error")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let stopped = ProcessGroup(self.dir.path().to_owned())
            .stop(self.unfinished.get())
            .and_then(|()| {
                if self.unfinished.get() == 0 {
                    Ok(())
                } else {
                    Err(io::Error::other("an asynchronous operation did not finish"))
                }
            });
        if let Err(error) = stopped {
            self.dir.disable_cleanup(true);
            let message = format!(
                "cannot confirm fixture shutdown; preserving {}: {error}",
                self.dir.path().display(),
            );
            if std::thread::panicking() {
                eprintln!("{message}");
            } else {
                panic!("{message}");
            }
        }
    }
}

/// Stops and reaps the CLI if a test fails before it exits.
pub(super) struct CliGuard(pub(super) Option<std::process::Child>);

impl CliGuard {
    pub(super) fn wait(mut self) -> std::process::Output {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.0.as_mut().unwrap().try_wait().unwrap().is_none() {
            assert!(
                Instant::now() < deadline,
                "the CLI did not exit after the signal"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        self.0.take().unwrap().wait_with_output().unwrap()
    }
}

impl Drop for CliGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            if !matches!(child.try_wait(), Ok(Some(_))) {
                let _ = rustix::process::kill_process(
                    rustix::process::Pid::from_child(&child),
                    rustix::process::Signal::TERM,
                );
                let deadline = Instant::now() + Duration::from_secs(5);
                while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                if !matches!(child.try_wait(), Ok(Some(_))) {
                    let _ = child.kill();
                }
            }
            if let Err(error) = child.wait() {
                if std::thread::panicking() {
                    eprintln!("reaping the test CLI failed: {error}");
                } else {
                    panic!("reaping the test CLI failed: {error}");
                }
            }
        }
    }
}
