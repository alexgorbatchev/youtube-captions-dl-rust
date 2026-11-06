use super::run;
use cli_command_kit::Mode;
use std::io::{self, Write};

fn registration_error() -> io::Error {
    use tokio::signal::unix::{SignalKind, signal};
    signal(SignalKind::from_raw(-1)).expect_err("Tokio must reject an invalid signal")
}

#[tokio::test]
async fn registration_failure_reports_once_and_stops_before_command_execution() {
    for (mode, prefix) in [(Mode::Human, "[ERROR]"), (Mode::Agent, "ERR:")] {
        let error = registration_error();
        let expected = format!("{prefix} registering signal handlers: {error}\n");
        let mut output = Vec::new();
        let mut diagnostics = Vec::new();
        let args = std::iter::from_fn(|| panic!("command must not run without signal handlers"));
        let status = run(Err(error), args, &mut output, &mut diagnostics, mode).await;
        assert_eq!(status, 1);
        assert!(output.is_empty());
        assert_eq!(String::from_utf8(diagnostics).unwrap(), expected);
    }
}

#[tokio::test]
async fn registration_failure_still_fails_when_diagnostics_cannot_be_written() {
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut output = Vec::new();
    let status = run(
        Err(registration_error()),
        ["youtube-captions-dl".into(), "--version".into()],
        &mut output,
        &mut Closed,
        Mode::Agent,
    )
    .await;
    assert_eq!(status, 1);
    assert!(output.is_empty());
}
