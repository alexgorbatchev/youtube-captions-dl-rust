#![cfg(unix)]

use cli_command_kit::Mode;
use std::{
    ffi::OsString,
    fs,
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use youtube_captions_dl::{Cache, Captions, Client, FetchError, Runtime};

struct Fixture {
    dir: tempfile::TempDir,
    runtime: Runtime,
}

impl Fixture {
    fn new(script: &str) -> Self {
        let dir = tempfile::tempdir_in(concat!(env!("CARGO_MANIFEST_DIR"), "/.tmp")).unwrap();
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
        }
    }

    fn metadata(metadata: serde_json::Value) -> Self {
        let json = metadata.to_string().replace('\'', "'\\''");
        Self::new(&format!("printf '%s\\n' '{json}'"))
    }
}

async fn run(
    args: &[&str],
    mode: Mode,
    token: &CancellationToken,
    runtime: Option<&Runtime>,
) -> (u8, String, String) {
    let mut output = Vec::new();
    let mut errors = Vec::new();
    let status = youtube_captions_dl::entry(
        args.iter().map(OsString::from),
        &mut output,
        &mut errors,
        mode,
        token,
        runtime,
    )
    .await;
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
        let (status, output, errors) = run(&args, mode, &token, Some(&fixture.runtime)).await;
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
            run(&args, mode, &token, Some(&fixture.runtime)).await,
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
            .runtime
            .client
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
            Some(&fixture.runtime),
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
            Some(&fixture.runtime),
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
            .runtime
            .client
            .fetch("tG1CSRaJhKQ", &CancellationToken::new())
            .await;
        thread.join().unwrap();
        assert!(result.is_err());
        assert!(!fixture.runtime.cache.directory.exists());
    }
    let mut fixture = Fixture::new("exec /bin/sleep 5");
    fixture.runtime.client.deadline = Duration::from_millis(30);
    let error = fixture
        .runtime
        .client
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
    let error = fixture
        .runtime
        .client
        .fetch("tG1CSRaJhKQ", &token)
        .await
        .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<FetchError>(),
        Some(FetchError::Cancelled)
    ));
    let (status, output, errors) = run(
        &["youtube-captions-dl", "caption", "get", "tG1CSRaJhKQ"],
        Mode::Agent,
        &token,
        Some(&fixture.runtime),
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
                youtube_captions_dl::entry(
                    args.iter().map(OsString::from),
                    &mut Broken,
                    &mut errors,
                    mode,
                    &token,
                    Some(&fixture.runtime)
                )
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
