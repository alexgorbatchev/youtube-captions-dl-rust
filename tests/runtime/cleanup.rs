use super::*;

#[test]
fn group_exit_waits_for_the_zombie_to_be_reaped() {
    let fixture = Fixture::new("dir=${0%/*}\necho $$ > \"$dir/leader.pid\"\nexit 0");
    let dir = fixture.dir.path().to_owned();
    let child = std::process::Command::new(dir.join("yt-dlp"))
        .process_group(0)
        .spawn()
        .unwrap();
    let mut cli = CliGuard(Some(child));
    let leader = rustix::process::Pid::from_child(cli.0.as_ref().unwrap());
    rustix::process::waitid(
        rustix::process::WaitId::Pid(leader),
        rustix::process::WaitIdOptions::EXITED | rustix::process::WaitIdOptions::NOWAIT,
    )
    .unwrap();
    let probe = rustix::process::test_kill_process_group(leader);
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let observer = std::thread::spawn(move || {
        result_tx.send(ProcessGroup(dir).wait_for_exit()).unwrap();
    });
    let premature = result_rx.recv_timeout(Duration::from_millis(100));
    cli.0.as_mut().unwrap().wait().unwrap();
    let waited_until_reaped = matches!(premature, Err(std::sync::mpsc::RecvTimeoutError::Timeout));
    let result =
        premature.unwrap_or_else(|_| result_rx.recv_timeout(Duration::from_secs(5)).unwrap());
    observer.join().unwrap();
    drop(cli);
    drop(fixture);
    #[cfg(target_os = "macos")]
    assert_eq!(probe, Err(rustix::io::Errno::PERM));
    #[cfg(not(target_os = "macos"))]
    assert!(probe.is_ok());
    assert!(
        waited_until_reaped,
        "an unreaped group is not an error or a completed shutdown"
    );
    result.unwrap();
    assert_eq!(
        rustix::process::test_kill_process_group(leader),
        Err(rustix::io::Errno::SRCH)
    );
}

#[test]
fn fixture_shutdown_stops_the_group_before_removing_scratch() {
    for stubborn in [false, true] {
        let fixture = terminating_stand_in(stubborn);
        let dir = fixture.dir.path().to_owned();
        let child = std::process::Command::new(dir.join("yt-dlp"))
            .process_group(0)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let cli = CliGuard(Some(child));
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(wait_for_file(&dir.join("ready")));
        let leader = ProcessGroup(dir.clone()).leader().unwrap().unwrap();
        let reaper = std::thread::spawn(move || cli.wait());
        let started = Instant::now();
        drop(fixture);
        let output = reaper.join().unwrap();
        assert_eq!(output.status.code(), Some(143), "the leader handled TERM");
        if stubborn {
            assert!(started.elapsed() >= Duration::from_millis(300));
        }
        assert_eq!(
            rustix::process::test_kill_process_group(leader),
            Err(rustix::io::Errno::SRCH),
            "every process exited before scratch removal"
        );
        assert!(!dir.exists());
    }
}

#[test]
fn failed_test_stops_the_cli_before_removing_its_scratch() {
    let fixture = terminating_stand_in(false);
    let dir = fixture.dir.path().to_owned();
    let group = ProcessGroup(dir.clone());
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_youtube-captions-dl"))
        .args(["caption", "get", "tG1CSRaJhKQ"])
        .env("AGENT", "1")
        .env("XDG_CACHE_HOME", dir.join("xdg"))
        .env("PATH", &dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let cli = CliGuard(Some(child));
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(wait_for_file(&dir.join("ready")));
    let failed = std::panic::catch_unwind(move || {
        let _cli = cli;
        panic!("simulate a failed assertion while the CLI is running");
    });
    assert!(failed.is_err());
    assert!(
        dir.join("leader-term").exists(),
        "shutdown allowed fixture cleanup"
    );
    group
        .wait_for_exit()
        .expect("shutdown confirmed every process exited");
    drop(fixture);
    assert!(!dir.exists(), "scratch was removed after shutdown");
}

#[test]
fn uncertain_process_ownership_preserves_fixture_scratch() {
    for invalid in ["not a process ID", "0", "-1"] {
        let fixture = Fixture::new("exit 0");
        let dir = fixture.dir.path().to_owned();
        fs::write(dir.join("leader.pid"), invalid).unwrap();
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(fixture)));
        assert!(failed.is_err());
        assert!(dir.join("yt-dlp").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn unwinding_before_pid_publication_preserves_live_process_scratch() {
    let mut fixture = Fixture::new(
        r#"dir=${0%/*}
trap 'while [ ! -f "$dir/allow-exit" ]; do /bin/sleep 0.01; done; exit 143' TERM
echo $$ > "$dir/ready"
while :; do /bin/sleep 0.05; done"#,
    );
    fixture.runtime.client.runner.termination_grace = Duration::from_secs(5);
    let dir = fixture.dir.path().to_owned();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut leader = None;
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let fixture = fixture;
        runtime.block_on(async {
            let token = CancellationToken::new();
            tokio::join!(fixture.fetch("tG1CSRaJhKQ", &token), async {
                wait_for_file(&dir.join("ready")).await;
                leader = rustix::process::Pid::from_raw(
                    fs::read_to_string(dir.join("ready"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap(),
                );
                assert!(!dir.join("leader.pid").exists());
                panic!("simulate an assertion before PID publication");
            });
        });
    }));
    let retained = dir.join("yt-dlp").exists();
    let leader = leader.expect("the stand-in started before the panic");
    let was_running = rustix::process::test_kill_process_group(leader).is_ok();
    if retained {
        fs::write(dir.join("allow-exit"), "").unwrap();
    } else {
        let _ = rustix::process::kill_process_group(leader, rustix::process::Signal::KILL);
    }
    runtime.shutdown_timeout(Duration::from_secs(10));
    let stopped = rustix::process::test_kill_process_group(leader);
    if retained && stopped == Err(rustix::io::Errno::SRCH) {
        fs::remove_dir_all(&dir).unwrap();
    }
    assert!(failed.is_err());
    assert_eq!(stopped, Err(rustix::io::Errno::SRCH));
    assert!(
        was_running,
        "the reaper had not yet finished during unwinding"
    );
    assert!(
        retained,
        "scratch must outlive the unconfirmed child shutdown"
    );
}
