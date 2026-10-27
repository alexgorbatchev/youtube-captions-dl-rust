use crate::{Captions, parse_video_id};
use anyhow::{Context, Result, bail};
use std::{ffi::OsStr, fs, io::Write, path::PathBuf};

pub fn cache_directory(xdg: Option<&OsStr>, home: Option<PathBuf>) -> Result<PathBuf> {
    let root = if let Some(xdg) = xdg.filter(|value| !value.is_empty()) {
        let path = PathBuf::from(xdg);
        if !path.is_absolute() {
            bail!("XDG_CACHE_HOME must be absolute");
        }
        path
    } else {
        home.context("cannot resolve home directory")?
            .join(".cache")
    };
    Ok(root.join("youtube-captions-dl"))
}

pub struct Cache {
    pub directory: PathBuf,
}

impl Cache {
    pub fn new() -> Result<Self> {
        Ok(Self {
            directory: cache_directory(
                std::env::var_os("XDG_CACHE_HOME").as_deref(),
                dirs::home_dir(),
            )?,
        })
    }

    fn path(&self, video_id: &str) -> Result<PathBuf> {
        let id = parse_video_id(video_id)?;
        Ok(self.directory.join(format!("{id}.v9.json")))
    }

    pub fn load(&self, video_id: &str) -> Result<Option<Captions>> {
        let bytes = match fs::read(self.path(video_id)?) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let captions: Captions =
            serde_json::from_slice(&bytes).context("decoding caption cache")?;
        if captions.video_id != parse_video_id(video_id)?
            || captions.title.trim().is_empty()
            || captions.cues.is_empty()
            || captions.cues.iter().any(|cue| cue.trim().is_empty())
        {
            bail!("caption cache contents are invalid");
        }
        Ok(Some(captions))
    }

    pub fn save(&self, captions: &Captions) -> Result<()> {
        let destination = self.path(&captions.video_id)?;
        fs::create_dir_all(&self.directory)?;
        let mut staged = tempfile::NamedTempFile::new_in(&self.directory)?;
        serde_json::to_writer(&mut staged, captions)?;
        staged.write_all(b"\n")?;
        staged.as_file().sync_all()?;
        staged.persist(destination).map_err(|error| error.error)?;
        Ok(())
    }
}
