use std::io::{self, Write};
use tokio_util::sync::CancellationToken;

/// Interrupt and termination requests, registered before any work starts.
///
/// `cli-deps` runs children in their own session, so terminal signals reach only
/// this process; cancelling the token lets the runner stop them gracefully.
struct Signals {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    #[cfg(windows)]
    interrupt: tokio::signal::windows::CtrlC,
}

impl Signals {
    fn register() -> io::Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            Ok(Self {
                interrupt: signal(SignalKind::interrupt())?,
                terminate: signal(SignalKind::terminate())?,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                interrupt: tokio::signal::windows::ctrl_c()?,
            })
        }
    }

    async fn cancel_on_signal(mut self, cancel: CancellationToken) {
        #[cfg(unix)]
        tokio::select! {
            _ = self.interrupt.recv() => {}
            _ = self.terminate.recv() => {}
        }
        #[cfg(windows)]
        self.interrupt.recv().await;
        cancel.cancel();
    }
}

#[tokio::main]
async fn main() {
    let mode = cli_command_kit::Mode::from_env();
    let signals = match Signals::register() {
        Ok(signals) => signals,
        Err(error) => {
            let _ = writeln!(
                io::stderr(),
                "{} registering signal handlers: {}",
                if mode == cli_command_kit::Mode::Agent {
                    "ERR:"
                } else {
                    "[ERROR]"
                },
                cli_command_kit::display_message(&error.to_string())
            );
            std::process::exit(1);
        }
    };
    let cancel = CancellationToken::new();
    let handler = tokio::spawn(signals.cancel_on_signal(cancel.clone()));
    let status = youtube_captions_dl::entry(
        std::env::args_os(),
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
        mode,
        &cancel,
        None,
    )
    .await;
    handler.abort();
    std::process::exit(i32::from(status));
}
