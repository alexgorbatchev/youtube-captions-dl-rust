mod cache;
mod cli;
mod transcript;

pub use cache::{Cache, cache_directory};
pub use cli::{Runtime, command, entry, help_options};
pub use transcript::{CaptionTrack, format_cues, parse_transcript, parse_video_id, select_track};

use anyhow::{Context, Result, bail};
use cli_deps::{CommandRunner, ProcessRunner};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("operation cancelled")]
    Cancelled,
    #[error("caption retrieval exceeded its deadline")]
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Captions {
    pub video_id: String,
    pub title: String,
    pub language: String,
    pub generated: bool,
    pub cues: Vec<String>,
}

pub struct Client {
    pub runner: ProcessRunner,
    pub http: reqwest::Client,
    pub deadline: Duration,
}

impl Client {
    pub fn new() -> Result<Self> {
        Ok(Self {
            runner: ProcessRunner::default(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?,
            deadline: Duration::from_secs(30),
        })
    }

    pub async fn fetch(&self, video_id: &str, cancel: &CancellationToken) -> Result<Captions> {
        let video_id = parse_video_id(video_id)?;
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(FetchError::Cancelled.into()),
            result = tokio::time::timeout(self.deadline, self.fetch_inner(&video_id, cancel)) => {
                result.map_err(|_| FetchError::Timeout)?
            }
        }
    }

    async fn fetch_inner(&self, video_id: &str, cancel: &CancellationToken) -> Result<Captions> {
        let arguments = [
            "--ignore-config",
            "--no-plugin-dirs",
            "--no-cache-dir",
            "--no-playlist",
            "--simulate",
            "--dump-single-json",
            "--ignore-no-formats-error",
            "--no-warnings",
            "--",
            &format!("https://www.youtube.com/watch?v={video_id}"),
        ]
        .map(str::to_owned);
        let bytes = self
            .runner
            .run("yt-dlp", &arguments, cancel)
            .await
            .context("extracting caption metadata with yt-dlp")?;
        let metadata: serde_json::Value =
            serde_json::from_slice(&bytes).context("decoding yt-dlp metadata")?;
        if metadata.get("id").and_then(serde_json::Value::as_str) != Some(video_id) {
            bail!("metadata does not identify the requested video");
        }
        let title = metadata
            .get("title")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .context("video title is missing")?
            .to_owned();
        let track = select_track(&metadata)?;
        let body = self
            .http
            .get(track.url)
            .headers(track.headers)
            .send()
            .await
            .context("fetching captions")?
            .error_for_status()
            .context("caption server rejected request")?
            .text()
            .await
            .context("reading captions")?;
        let cues = parse_transcript(&body)?;
        Ok(Captions {
            video_id: video_id.into(),
            title,
            language: track.language,
            generated: track.generated,
            cues,
        })
    }
}
