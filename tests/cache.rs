use std::{ffi::OsStr, fs, path::PathBuf};
use youtube_captions_dl::{Cache, Captions, cache_directory};

#[test]
fn cache_round_trip_is_atomic_private_and_validated() {
    let directory = tempfile::tempdir_in(concat!(env!("CARGO_MANIFEST_DIR"), "/.tmp")).unwrap();
    let cache = Cache {
        directory: directory.path().join("captions"),
    };
    let mut captions = Captions {
        video_id: "tG1CSRaJhKQ".into(),
        title: "Talk".into(),
        language: "en".into(),
        generated: false,
        cues: vec!["Hello".into()],
    };
    assert!(cache.load(&captions.video_id).unwrap().is_none());
    cache.save(&captions).unwrap();
    assert_eq!(
        cache.load(&captions.video_id).unwrap(),
        Some(captions.clone())
    );
    let path = cache.directory.join("tG1CSRaJhKQ.v9.json");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    captions.title = "Replacement".into();
    cache.save(&captions).unwrap();
    assert_eq!(
        cache.load(&captions.video_id).unwrap(),
        Some(captions.clone())
    );
    assert_eq!(fs::read_dir(&cache.directory).unwrap().count(), 1);
    fs::write(&path, b"broken").unwrap();
    assert!(cache.load(&captions.video_id).is_err());
    for changed in [
        serde_json::json!({"video_id":"differentid","title":"Talk","language":"en","generated":false,"cues":["a"]}),
        serde_json::json!({"video_id":"tG1CSRaJhKQ","title":"","language":"en","generated":false,"cues":["a"]}),
        serde_json::json!({"video_id":"tG1CSRaJhKQ","title":"Talk","language":"en","generated":false,"cues":[]}),
        serde_json::json!({"video_id":"tG1CSRaJhKQ","title":"Talk","language":"en","generated":false,"cues":[" "]}),
    ] {
        fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(cache.load(&captions.video_id).is_err());
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(cache.load(&captions.video_id).is_err());
    assert!(cache.save(&captions).is_err());
    assert!(cache.load("../../invalid").is_err());
    let blocked = directory.path().join("blocked");
    fs::write(&blocked, b"keep").unwrap();
    assert!(
        Cache {
            directory: blocked.clone()
        }
        .save(&captions)
        .is_err()
    );
    assert_eq!(fs::read(blocked).unwrap(), b"keep");
}

#[test]
fn cache_directory_resolves_xdg_without_relative_paths() {
    let home = std::env::current_dir().unwrap();
    let root = home.join("example-cache");
    assert_eq!(
        cache_directory(Some(root.as_os_str()), None).unwrap(),
        root.join("youtube-captions-dl")
    );
    assert_eq!(
        cache_directory(Some(OsStr::new("")), Some(home.clone())).unwrap(),
        home.join(".cache/youtube-captions-dl")
    );
    assert!(cache_directory(Some(OsStr::new("relative")), Some(home)).is_err());
    assert!(cache_directory(None, None).is_err());
    assert!(cache_directory(None, Some(PathBuf::from("/home/example"))).is_ok());
}
