use anyhow::{Context, Result, bail};
use serde::Deserialize;
use url::Url;

fn valid_id(value: &str) -> bool {
    value.len() == 11
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn parse_video_id(input: &str) -> Result<String> {
    let input = input.trim();
    if valid_id(input) {
        return Ok(input.into());
    }
    let url = Url::parse(input).context("expected a YouTube URL or11-character video ID")?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("expected an HTTP or HTTPS YouTube URL");
    }
    let host = url.host_str().unwrap_or_default();
    let parts: Vec<_> = url.path_segments().into_iter().flatten().collect();
    let id = match host {
        "youtu.be" => parts.first().copied().map(str::to_owned),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" => {
            if url.path() == "/watch" {
                url.query_pairs()
                    .find(|(key, _)| key == "v")
                    .map(|(_, value)| value.into_owned())
            } else if parts
                .first()
                .is_some_and(|part| matches!(*part, "shorts" | "live" | "embed"))
            {
                parts.get(1).copied().map(str::to_owned)
            } else {
                None
            }
        }
        _ => None,
    };
    id.filter(|id| valid_id(id))
        .context("could not extract a valid YouTube video ID")
}

pub struct CaptionTrack {
    pub url: Url,
    pub language: String,
    pub generated: bool,
    pub headers: reqwest::header::HeaderMap,
}

pub fn select_track(metadata: &serde_json::Value) -> Result<CaptionTrack> {
    for (field, generated) in [("subtitles", false), ("automatic_captions", true)] {
        if let Some(languages) = metadata.get(field).and_then(serde_json::Value::as_object) {
            for (language, formats) in languages {
                for format in formats.as_array().into_iter().flatten() {
                    if format.get("ext").and_then(serde_json::Value::as_str) != Some("srv1") {
                        continue;
                    }
                    if let Some(url) = format
                        .get("url")
                        .and_then(serde_json::Value::as_str)
                        .and_then(|value| Url::parse(value).ok())
                        && matches!(url.scheme(), "http" | "https")
                    {
                        return Ok(CaptionTrack {
                            url,
                            language: language.into(),
                            generated,
                            headers: format
                                .get("http_headers")
                                .or_else(|| metadata.get("http_headers"))
                                .map(|headers| -> Result<_> {
                                    let fields: std::collections::HashMap<String, String> =
                                        serde_json::from_value(headers.clone())?;
                                    Ok(reqwest::header::HeaderMap::try_from(&fields)?)
                                })
                                .transpose()
                                .context("decoding caption request headers")?
                                .unwrap_or_default(),
                        });
                    }
                }
            }
        }
    }
    bail!("no usable XML caption track is available")
}

#[derive(Deserialize)]
struct Transcript {
    #[serde(rename = "text", default)]
    entries: Vec<Text>,
}

#[derive(Deserialize)]
struct Text {
    #[serde(rename = "$text", default)]
    text: String,
}

pub fn parse_transcript(body: &str) -> Result<Vec<String>> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(body);
    loop {
        match reader.read_event().context("reading caption XML root")? {
            Event::Decl(_) | Event::Comment(_) => continue,
            Event::Text(text) if text.as_ref().bytes().all(|byte| byte.is_ascii_whitespace()) => {
                continue;
            }
            Event::Start(root) if root.name().as_ref() == "transcript" => {
                reader
                    .read_to_end(root.name())
                    .context("reading complete transcript")?;
                break;
            }
            Event::Empty(root) if root.name().as_ref() == "transcript" => break,
            _ => bail!("expected one transcript XML document"),
        }
    }
    loop {
        match reader.read_event().context("reading caption XML ending")? {
            Event::Comment(_) => continue,
            Event::Text(text) if text.as_ref().bytes().all(|byte| byte.is_ascii_whitespace()) => {
                continue;
            }
            Event::Eof => break,
            _ => bail!("unexpected data after transcript XML"),
        }
    }
    let document: Transcript = quick_xml::de::from_str(body).context("decoding caption XML")?;
    let cues: Vec<_> = document
        .entries
        .into_iter()
        .map(|entry| {
            let decoded = html_escape::decode_html_entities(&entry.text);
            let fragment = scraper::Html::parse_fragment(&decoded);
            let text = fragment.root_element().text().collect::<String>();
            html_escape::decode_html_entities(&text)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|value| !value.is_empty())
        .collect();
    if cues.is_empty() {
        bail!("caption transcript is empty");
    }
    Ok(cues)
}

pub fn format_cues(cues: &[String], width: Option<usize>) -> String {
    let mut segments: Vec<String> = Vec::new();
    let mut sound = false;
    for cue in cues {
        let bracket = cue.starts_with('[') && cue.ends_with(']');
        if segments.is_empty() || bracket || cue.starts_with(">>") || sound {
            segments.push(cue.clone());
        } else {
            let last = segments.last_mut().unwrap();
            last.push(' ');
            last.push_str(cue);
        }
        sound = bracket;
    }
    segments
        .into_iter()
        .map(|segment| match width {
            Some(width) => textwrap::fill(
                &segment,
                textwrap::Options::new(width.max(1)).break_words(false),
            ),
            None => segment,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
