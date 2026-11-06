#![cfg(unix)]

mod support;

#[path = "runtime/cleanup.rs"]
mod cleanup;

use cli_command_kit::Mode;
use std::{
    ffi::OsString,
    fs,
    io::{self, Write},
    os::unix::process::CommandExt,
    path::Path,
    time::{Duration, Instant},
};
use support::{CliGuard, Fixture, ProcessGroup};
use tokio_util::sync::CancellationToken;
use youtube_captions_dl::{Cache, Captions, FetchError};

async fn run(
    args: &[&str],
    mode: Mode,
    token: &CancellationToken,
    fixture: Option<&Fixture>,
) -> (u8, String, String) {
    let mut output = Vec::new();
    let mut errors = Vec::new();
    let work = youtube_captions_dl::entry(
        args.iter().map(OsString::from),
        &mut output,
        &mut errors,
        mode,
        token,
        fixture.map(|fixture| &fixture.runtime),
    );
    let status = match fixture {
        Some(fixture) => fixture.track(work).await,
        None => work.await,
    };
    (
        status,
        String::from_utf8(output).unwrap(),
        String::from_utf8(errors).unwrap(),
    )
}

fn metadata(url: &str) -> serde_json::Value {
    serde_json::json!({"id":"tG1CSRaJhKQ","title":"Caption talk", "subtitles":{"en":[{"ext":"srv1","url":url}]}})
}

fn server(status: u16, body: &str) -> (String, std::thread::JoinHandle<()>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}/captions", server.server_addr());
    let body = body.to_owned();
    let thread = std::thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .expect("caption request");
        assert_eq!(request.method(), &tiny_http::Method::Get);
        assert_eq!(request.url(), "/captions");
        request
            .respond(tiny_http::Response::from_string(body).with_status_code(status))
            .unwrap();
    });
    (url, thread)
}

#[tokio::test]
async fn native_process_and_http_feed_cache_and_dual_mode_output() {
    for mode in [Mode::Human, Mode::Agent] {
        let (url, thread) = server(
            200,
            "<transcript><text>Hello</text><text>world.</text><text>[Music]</text></transcript>",
        );
        let fixture = Fixture::metadata(metadata(&url));
        let token = CancellationToken::new();
        let args = [
            "youtube-captions-dl",
            "caption",
            "get",
            "https://youtu.be/tG1CSRaJhKQ",
        ];
        let (status, output, errors) = run(&args, mode, &token, Some(&fixture)).await;
        thread.join().unwrap();
        assert_eq!(status, 0, "{errors}");
        assert!(errors.is_empty());
        if mode == Mode::Agent {
            let value: serde_json::Value = serde_json::from_str(&output).unwrap();
            assert_eq!(value["text"], "Hello world.\n[Music]");
            assert_eq!(value["title"], "Caption talk");
            assert_eq!(value["language"], "en");
            assert_eq!(value["generated"], false);
            assert_eq!(output.lines().count(), 1);
        } else {
            let lines: Vec<_> = output.lines().collect();
            assert_eq!(lines[0], "Caption talk");
            assert!(!lines[1].is_empty() && lines[1].chars().all(|c| c == '-'));
            assert_eq!(&lines[2..], &["Hello world.", "[Music]"]);
        }
        fs::remove_file(fixture.dir.path().join("yt-dlp")).unwrap();
        assert_eq!(
            run(&args, mode, &token, Some(&fixture)).await,
            (0, output, String::new())
        );
    }
}

#[tokio::test]
async fn subtitle_requests_preserve_native_extractor_header_policy() {
    for override_track in [false, true] {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}/captions", server.server_addr());
        let mut value = metadata(&url);
        value["http_headers"] = serde_json::json!({"User-Agent":"metadata-agent"});
        if override_track {
            value["subtitles"]["en"][0]["http_headers"] =
                serde_json::json!({"User-Agent":"track-agent"});
        }
        let fixture = Fixture::metadata(value);
        let thread = std::thread::spawn(move || {
            let request = server
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap();
            let agent = request
                .headers()
                .iter()
                .find(|header| header.field.equiv("User-Agent"))
                .map(|header| header.value.as_str().to_owned());
            request
                .respond(tiny_http::Response::from_string(
                    "<transcript><text>Caption</text></transcript>",
                ))
                .unwrap();
            agent
        });
        fixture
            .fetch("tG1CSRaJhKQ", &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            thread.join().unwrap().as_deref(),
            Some(if override_track {
                "track-agent"
            } else {
                "metadata-agent"
            })
        );
    }
    let mut value = metadata("http://127.0.0.1:1/captions");
    value["http_headers"] = serde_json::json!({"Bad Header":"bad"});
    assert!(youtube_captions_dl::select_track(&value).is_err());
}

#[tokio::test]
async fn cache_failures_are_nonfatal_warnings_and_do_not_replace_blockers() {
    for mode in [Mode::Agent, Mode::Human] {
        let (url, thread) = server(200, "<transcript><text>Recovered</text></transcript>");
        let fixture = Fixture::metadata(metadata(&url));
        fs::create_dir_all(&fixture.runtime.cache.directory).unwrap();
        let path = fixture.runtime.cache.directory.join("tG1CSRaJhKQ.v9.json");
        fs::create_dir(&path).unwrap();
        let (status, output, errors) = run(
            &["youtube-captions-dl", "caption", "get", "tG1CSRaJhKQ"],
            mode,
            &CancellationToken::new(),
            Some(&fixture),
        )
        .await;
        thread.join().unwrap();
        assert_eq!(status, 0);
        assert!(output.contains("Recovered"));
        assert_eq!(errors.lines().count(), 2);
        assert!(errors.contains("reading cache failed"));
        assert!(errors.contains("writing cache failed"));
        assert!(path.is_dir());
    }
}

#[tokio::test]
async fn extraction_failures_are_typed_and_http_failures_are_reported_once() {
    for fixture in [
        Fixture::new("printf '%s\\n' 'ERROR: denied' >&2\nexit 7"),
        Fixture::new("printf '%s\\n' 'not-json'"),
        Fixture::metadata(serde_json::json!({"id":"differentid","title":"Wrong"})),
        Fixture::metadata(serde_json::json!({"id":"tG1CSRaJhKQ","title":" "})),
        Fixture::metadata(serde_json::json!({"id":"tG1CSRaJhKQ","title":"Talk"})),
    ] {
        let (status, output, errors) = run(
            &["youtube-captions-dl", "caption", "get", "tG1CSRaJhKQ"],
            Mode::Agent,
            &CancellationToken::new(),
            Some(&fixture),
        )
        .await;
        assert_eq!(status, 1);
        assert!(output.is_empty());
        assert_eq!(errors.lines().count(), 1, "{errors}");
        assert!(errors.starts_with("ERR:"));
        assert!(!errors.contains("Usage:"));
    }
    for (status, body) in [(403, "denied"), (200, "<html>bad</html>")] {
        let (url, thread) = server(status, body);
        let fixture = Fixture::metadata(metadata(&url));
        let result = fixture
            .fetch("tG1CSRaJhKQ", &CancellationToken::new())
            .await;
        thread.join().unwrap();
        assert!(result.is_err());
        assert!(!fixture.runtime.cache.directory.exists());
    }
    let mut fixture = Fixture::new("exec /bin/sleep 5");
    fixture.runtime.client.deadline = Duration::from_millis(30);
    let error = fixture
        .fetch("tG1CSRaJhKQ", &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<FetchError>(),
        Some(FetchError::Timeout)
    ));
    fixture.runtime.client.deadline = Duration::from_secs(10);
    let token = CancellationToken::new();
    let trigger = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(30)).await;
        trigger.cancel();
    });
    let error = fixture.fetch("tG1CSRaJhKQ", &token).await.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<FetchError>(),
        Some(FetchError::Cancelled)
    ));
    let (status, output, errors) = run(
        &["youtube-captions-dl", "caption", "get", "tG1CSRaJhKQ"],
        Mode::Agent,
        &token,
        Some(&fixture),
    )
    .await;
    assert_eq!(status, 130);
    assert!(output.is_empty());
    assert_eq!(errors, "ERR: operation cancelled\n");
}

struct Broken;
impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("writer unavailable"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn metadata_and_validation_are_offline_and_handle_output_failures() {
    for mode in [Mode::Human, Mode::Agent] {
        let token = CancellationToken::new();
        assert_eq!(
            run(&["youtube-captions-dl", "--version"], mode, &token, None).await,
            (0, "0.1.0\n".into(), String::new())
        );
        for args in [
            vec!["youtube-captions-dl", "--help"],
            vec!["youtube-captions-dl", "caption", "--help"],
            vec!["youtube-captions-dl", "caption", "get", "--help"],
            vec!["youtube-captions-dl", "help", "caption", "get"],
            vec!["youtube-captions-dl", "skill", "--help"],
        ] {
            let (status, output, errors) = run(&args, mode, &token, None).await;
            assert_eq!(status, 0, "{errors}");
            assert!(errors.is_empty());
            assert_eq!(output.starts_with("ALERT: Agents must read `AGENT=1 youtube-captions-dl skill` before using this tool.\n"), mode == Mode::Agent);
            if args.contains(&"get") {
                let words = output.split_whitespace().collect::<Vec<_>>().join(" ");
                assert!(words.contains("YouTube URL or 11-character video ID"));
            }
        }
        for args in [
            vec!["youtube-captions-dl", "caption", "get", "invalid"],
            vec!["youtube-captions-dl", "skill", "extra"],
            vec!["youtube-captions-dl", "--version", "caption"],
            vec!["youtube-captions-dl", "--bad"],
        ] {
            let (status, output, errors) = run(&args, mode, &token, None).await;
            assert_eq!(status, 1);
            assert!(output.is_empty());
            assert!(errors.contains("error:"));
        }
        let fixture = Fixture::new("exit 1");
        fixture
            .runtime
            .cache
            .save(&Captions {
                video_id: "tG1CSRaJhKQ".into(),
                title: "Cached".into(),
                language: "en".into(),
                generated: false,
                cues: vec!["Hello".into()],
            })
            .unwrap();
        for args in [
            vec!["youtube-captions-dl", "skill"],
            vec!["youtube-captions-dl", "--version"],
            vec!["youtube-captions-dl", "--help"],
            vec!["youtube-captions-dl", "caption", "get", "tG1CSRaJhKQ"],
        ] {
            let mut errors = Vec::new();
            assert_eq!(
                fixture
                    .track(youtube_captions_dl::entry(
                        args.iter().map(OsString::from),
                        &mut Broken,
                        &mut errors,
                        mode,
                        &token,
                        Some(&fixture.runtime)
                    ))
                    .await,
                1
            );
            assert_eq!(String::from_utf8(errors).unwrap().lines().count(), 1);
        }
    }
    let human = run(
        &["youtube-captions-dl", "skill"],
        Mode::Human,
        &CancellationToken::new(),
        None,
    )
    .await;
    let agent = run(
        &["youtube-captions-dl", "skill"],
        Mode::Agent,
        &CancellationToken::new(),
        None,
    )
    .await;
    assert_eq!(human, agent);
    assert!(human.1.contains(
        "npx skills add alexgorbatchev/youtube-captions-dl --skill youtube-captions-dl\n"
    ));
}

#[test]
fn native_binary_respects_environment_and_reads_cache_offline() {
    let fixture = Fixture::new("exit 9");
    let xdg = fixture.dir.path().join("xdg");
    let cache = Cache {
        directory: xdg.join("youtube-captions-dl"),
    };
    cache
        .save(&Captions {
            video_id: "tG1CSRaJhKQ".into(),
            title: "Offline title".into(),
            language: "en".into(),
            generated: true,
            cues: vec!["Offline caption.".into()],
        })
        .unwrap();
    for agent in ["0", "true"] {
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_youtube-captions-dl"))
            .args(["caption", "get", "tG1CSRaJhKQ"])
            .env("AGENT", agent)
            .env("XDG_CACHE_HOME", &xdg)
            .env("PATH", fixture.dir.path())
            .output()
            .unwrap();
        assert!(result.status.success());
        assert!(result.stderr.is_empty());
        let output = String::from_utf8(result.stdout).unwrap();
        if agent == "true" {
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&output).unwrap()["text"],
                "Offline caption."
            );
        } else {
            assert!(output.starts_with("Offline title\n"));
        }
    }
    let version = std::process::Command::new(env!("CARGO_BIN_EXE_youtube-captions-dl"))
        .arg("-v")
        .env("XDG_CACHE_HOME", "relative")
        .output()
        .unwrap();
    assert!(version.status.success());
    assert_eq!(version.stdout, b"0.1.0\n");
    let invalid = std::process::Command::new(env!("CARGO_BIN_EXE_youtube-captions-dl"))
        .args(["caption", "get", "tG1CSRaJhKQ"])
        .env("XDG_CACHE_HOME", "relative")
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    assert!(
        String::from_utf8(invalid.stderr)
            .unwrap()
            .contains("XDG_CACHE_HOME must be absolute")
    );
}

#[test]
fn skill_covers_live_interface_and_its_exact_repository_destination() {
    let guide = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/skills/youtube-captions-dl/SKILL.md"
    ))
    .unwrap();
    let missing = cli_command_kit::skill_omissions(
        &youtube_captions_dl::command(),
        &youtube_captions_dl::help_options(),
        &guide,
    );
    assert!(missing.is_empty(), "{missing:?}");
    assert!(
        guide
            .lines()
            .any(|line| line == "name: youtube-captions-dl")
    );
}

/// A stand-in `yt-dlp` whose SIGTERM handler writes to stderr before recording
/// `leader-term` and exiting, so the marker proves the output pipes stayed open
/// through termination. With `stubborn`, it first starts a grandchild that does
/// the same with `grandchild-term` and keeps running, so only SIGKILL can end it.
fn terminating_stand_in(stubborn: bool) -> Fixture {
    let grandchild = if stubborn {
        r#"sh -c 'trap "echo grandchild stopping >&2; echo > \"$1/grandchild-term\"" TERM
echo $$ > "$1/grandchild.tmp" && mv "$1/grandchild.tmp" "$1/grandchild.pid"
while :; do sleep 0.05; done' sh "$dir" &
while [ ! -f "$dir/grandchild.pid" ]; do sleep 0.01; done"#
    } else {
        ""
    };
    Fixture::new(&format!(
        r#"PATH=/usr/bin:/bin
dir=$(dirname "$0")
echo $$ > "$dir/leader.tmp" && mv "$dir/leader.tmp" "$dir/leader.pid"
trap 'echo stopping >&2; echo > "$dir/leader-term"; exit 143' TERM
{grandchild}
echo > "$dir/ready"
while :; do sleep 0.05; done"#
    ))
}

async fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{} never appeared",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn cancellation_and_deadline_terminate_yt_dlp_before_killing_its_group() {
    let grace = Duration::from_millis(300);
    for timeout in [false, true] {
        let mut fixture = terminating_stand_in(true);
        let dir = fixture.dir.path().to_owned();
        let group = ProcessGroup(dir.clone());
        fixture.runtime.client.runner.termination_grace = grace;
        fixture.runtime.client.deadline = Duration::from_secs(if timeout { 2 } else { 30 });
        let token = CancellationToken::new();
        let started = Instant::now();
        let trigger = async {
            wait_for_file(&dir.join("ready")).await;
            if !timeout {
                token.cancel();
            }
            Instant::now()
        };
        let (result, ready) = tokio::join!(fixture.fetch("tG1CSRaJhKQ", &token), trigger);
        let returned = Instant::now();
        let error = result.unwrap_err();
        assert!(
            matches!(
                (timeout, error.downcast_ref::<FetchError>()),
                (false, Some(FetchError::Cancelled)) | (true, Some(FetchError::Timeout))
            ),
            "{error:#}"
        );
        assert!(dir.join("leader-term").exists(), "yt-dlp received SIGTERM");
        assert!(
            dir.join("grandchild-term").exists(),
            "the rest of the group received SIGTERM"
        );
        let signalled = if timeout {
            started + fixture.runtime.client.deadline
        } else {
            ready
        };
        assert!(
            returned - signalled >= grace,
            "the group kept its grace period before SIGKILL"
        );
        group.wait_for_exit().expect("SIGKILL ended the group");
    }
}

#[test]
fn interrupt_and_termination_signals_cancel_the_cli_and_stop_yt_dlp() {
    for signal in [rustix::process::Signal::INT, rustix::process::Signal::TERM] {
        let fixture = terminating_stand_in(false);
        let dir = fixture.dir.path().to_owned();
        let group = ProcessGroup(dir.clone());
        let child = std::process::Command::new(env!("CARGO_BIN_EXE_youtube-captions-dl"))
            .args(["caption", "get", "tG1CSRaJhKQ"])
            .env("AGENT", "1")
            .env("XDG_CACHE_HOME", dir.join("xdg"))
            .env("PATH", &dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let cli = CliGuard(Some(child));
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(wait_for_file(&dir.join("ready")));
        let pid = rustix::process::Pid::from_child(cli.0.as_ref().unwrap());
        rustix::process::kill_process(pid, signal).unwrap();
        let output = cli.wait();
        assert_eq!(output.status.code(), Some(130), "{signal:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"ERR: operation cancelled\n");
        assert!(dir.join("leader-term").exists(), "yt-dlp received SIGTERM");
        group.wait_for_exit().unwrap();
        assert!(!dir.join("xdg").exists(), "a cancelled run writes no cache");
    }
}
