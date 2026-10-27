`youtube-captions-dl` retrieves a YouTube video's title and readable captions for terminal users and automation. It caches successful retrievals so repeated reads work offline.

# What It Does

- **Captions:** Accepts video IDs and watch, short, live, embed, and short-link URLs.
- **Track selection:** Prefers manual captions and falls back to automatic captions.
- **Readable text:** Decodes markup, groups speaker and sound cues, and wraps human output.
- **Automation:** Emits a compact JSON object with `AGENT=1`.

# How It Works

- Run `youtube-captions-dl caption get <VIDEO>` with a YouTube URL or video ID.
- A cached copy is returned immediately when available.
- Otherwise, the tool discovers captions, downloads the selected transcript, caches it, and prints the result.

# How it Really Works

- Uncached retrieval invokes yt-dlp from `PATH` for metadata and makes an HTTP request for the first usable XML caption track. User yt-dlp configuration, plugins, media downloads, and its own filesystem cache are disabled.
- Manual captions take precedence over automatic captions. Track order comes from the extractor; there is no language-selection option.
- A 30-second deadline covers extraction and download together. YouTube access restrictions or challenge requirements can prevent a fetch.
- Cached cues live indefinitely in `$XDG_CACHE_HOME/youtube-captions-dl/<ID>.v9.json`, falling back to `~/.cache/youtube-captions-dl/`. Remove a video's file to refresh it. Cache files are atomically replaced and private on Unix.
- Cache warnings go to stderr and retrieval continues when possible. Human stdout contains the title, a terminal-width divider, and captions wrapped to 80 display columns. `AGENT=1` stdout contains `video_id`, `title`, `language`, `generated`, and unwrapped `text` as one JSON object.
- Exit 0 means success or help/version output, exit 1 means failure, and exit 130 means cancellation. An output failure can occur after a successful cache write.

# Installation

This Rust project has no published binary release. The [source repository](https://github.com/alexgorbatchev/youtube-captions-dl) identifies the Go tool; its releases and skill URL do not establish availability of this Rust implementation.

# Setup

- Install [yt-dlp](https://github.com/yt-dlp/yt-dlp#installation) on `PATH` for uncached retrieval. Follow its [YouTube JavaScript runtime guidance](https://github.com/yt-dlp/yt-dlp/wiki/EJS) when the extractor requires challenge support. Cache hits need no external binary or network.
- Set `XDG_CACHE_HOME` to an absolute directory when choosing a cache parent; empty/unset uses the resolved home directory's `.cache`.

# Quick Start

```sh
youtube-captions-dl caption get https://youtu.be/tG1CSRaJhKQ
AGENT=1 youtube-captions-dl caption get tG1CSRaJhKQ
youtube-captions-dl --version
youtube-captions-dl skill
```

Sample Output, from a local cached example at 40 terminal columns:

```text
Offline title
----------------------------------------
Offline caption.
```

# Options & Flags

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--help` | `-h` | `false` | Print help for the current command. |
| `--version` | `-v` | `false` | Print the raw version at the root; exclusive of other arguments. |

`caption get <VIDEO>` accepts one required URL or 11-character video ID and has no additional flags. `help [COMMAND]...` prints nested help. `skill` prints the install command and source skill URL offline; the Rust skill itself lives at [skills/youtube-captions-dl/SKILL.md](skills/youtube-captions-dl/SKILL.md).

# License

[MIT](LICENSE), Alex Gorbatchev.
