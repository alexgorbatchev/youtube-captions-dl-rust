use youtube_captions_dl::{format_cues, parse_transcript, parse_video_id, select_track};

#[test]
fn video_identifiers_accept_supported_urls_and_reject_lookalikes() {
    for input in [
        " tG1CSRaJhKQ ",
        "https://WWW.YOUTUBE.COM/watch?v=tG1CSRaJhKQ&list=WL",
        "https://youtu.be/tG1CSRaJhKQ?t=2",
        "https://music.youtube.com/watch?v=tG1CSRaJhKQ",
        "https://m.youtube.com/live/tG1CSRaJhKQ",
        "https://youtube.com/shorts/tG1CSRaJhKQ",
        "https://youtube.com/embed/tG1CSRaJhKQ",
    ] {
        assert_eq!(parse_video_id(input).unwrap(), "tG1CSRaJhKQ");
    }
    for input in [
        "",
        "https://evil.youtube.com/watch?v=tG1CSRaJhKQ",
        "https://youtube.com.evil.test/watch?v=tG1CSRaJhKQ",
        "ftp://youtube.com/watch?v=tG1CSRaJhKQ",
        "https://youtube.com/watch?v=!!!!!!!!!!!",
        "https://youtu.be/!!!!!!!!!!!",
        "https://youtube.com/?v=tG1CSRaJhKQ",
        "https://youtube.com/watch",
        "https://youtube.com/watch?v=short",
    ] {
        assert!(parse_video_id(input).is_err(), "{input}");
    }
}

#[test]
fn metadata_selection_preserves_manual_priority_and_provider_order() {
    let metadata = serde_json::json!({"title":"Talk", "automatic_captions":{"en":[{"ext":"srv1","url":"https://auto.test/sub"}]}, "subtitles":{"fr":[{"ext":"vtt","url":"https://unused.test"},{"ext":"srv1","url":"https://manual.test/sub?fmt=srv1&lang=fr"}],"en":[{"ext":"srv1","url":"https://later.test"}]}});
    let selected = select_track(&metadata).unwrap();
    assert_eq!(selected.language, "fr");
    assert!(!selected.generated);
    assert_eq!(
        selected.url.as_str(),
        "https://manual.test/sub?fmt=srv1&lang=fr"
    );
    let selected = select_track(&serde_json::json!({"automatic_captions":{"en":[{"ext":"srv1","url":"https://auto.test"}]}})).unwrap();
    assert!(selected.generated);
    for metadata in [
        serde_json::json!({}),
        serde_json::json!({"subtitles":{"en":[{"ext":"srv1","url":"file:///secret"}]}}),
        serde_json::json!({"subtitles":{"en":[{"ext":"vtt","url":"https://unused.test"}]}}),
    ] {
        assert!(select_track(&metadata).is_err());
    }
}

#[test]
fn captions_decode_markup_and_preserve_speaker_and_sound_cues() {
    let cues = parse_transcript(r#"<transcript><text>Hello &amp; welcome</text><text>This is &lt;i&gt;formatted&lt;/i&gt; text</text><text>&amp;gt;&amp;gt; Speaker line one.</text><text>Still the same speaker.</text><text>[applause]</text><text>[cheering]</text><text>Back to normal text.</text><text>   </text></transcript>"#).unwrap();
    assert_eq!(
        format_cues(&cues, Some(80)),
        "Hello & welcome This is formatted text\n>> Speaker line one. Still the same speaker.\n[applause]\n[cheering]\nBack to normal text."
    );
    assert!(parse_transcript("<transcript><text>broken</transcript>").is_err());
    assert!(parse_transcript("<wrong>empty</wrong>").is_err());
    assert!(parse_transcript("<wrong><text>wrong root</text></wrong>").is_err());
    assert!(
        parse_transcript(
            "<transcript><text>ok</text></transcript><transcript><text>extra</text></transcript>"
        )
        .is_err()
    );
    assert!(parse_transcript("<transcript/>").is_err());
    let text = format_cues(&["éé éé éé".into()], Some(5));
    assert_eq!(text, "éé éé\néé");
    assert_eq!(
        format_cues(&["A very long caption remains on one line".into()], None),
        "A very long caption remains on one line"
    );
}
