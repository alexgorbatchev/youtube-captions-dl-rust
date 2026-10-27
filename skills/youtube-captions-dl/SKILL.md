---
name: youtube-captions-dl
description: Retrieve a YouTube video's title and plain-text captions with youtube-captions-dl, including cached retrieval and machine-readable output.
author: alexgorbatchev
metadata:
  created_on: 2026-10-14 16:47
  last_modified: 2026-10-14 16:59
  status: current
---

# youtube-captions-dl

Use `AGENT=1 youtube-captions-dl caption get <VIDEO>` for one JSON object on stdout. `VIDEO` is a case-sensitive 11-character ASCII video ID (`A-Z`, `a-z`, `0-9`, `_`, `-`) or an HTTP/HTTPS YouTube URL: `/watch?v=`, `/shorts/`, `/live/`, `/embed/`, or `youtu.be/<ID>`. Hosts are `youtube.com`, `www.youtube.com`, `m.youtube.com`, `music.youtube.com`, and `youtu.be`; surrounding whitespace is trimmed.

## Commands

| Command | Inputs and behavior |
| :--- | :--- |
| `youtube-captions-dl` | Print root help when no arguments are supplied. |
| `youtube-captions-dl caption` | Group for reading captions; requires a subcommand. |
| `youtube-captions-dl caption get <VIDEO>` | Retrieve the title and captions, or read a validated cached copy. Required string `VIDEO` is validated before cache, subprocess, or HTTP access. |
| `youtube-captions-dl skill` | Print the Skills CLI install command and GitHub skill URL verbatim in either mode, offline. |
| `youtube-captions-dl help [COMMAND]...` | Print help for the root or a nested command. |
| `youtube-captions-dl caption help [COMMAND]...` | Print caption-group help or nested command help. |
| `youtube-captions-dl caption help get` | Print `caption get` help. |
| `youtube-captions-dl caption help help` | Print caption-group generated help-command help. |
| `youtube-captions-dl help help` | Print generated root help-command help. |
| `youtube-captions-dl help skill` | Print skill-command help. |

| Flag | Short | Type/default | Scope and behavior |
| :--- | :--- | :--- | :--- |
| `--help` | `-h` | bool/false | Every command; print help and exit. Agent help starts with the skill-reading alert. |
| `--version` | `-v` | bool/false | Root only; exclusive of other arguments. Print the raw version plus one newline. |

The generated `help` command also accepts the generated nested `help` path. Shell completion is not a public command.

## Retrieval and caching

The first usable manual caption track is selected in the extractor's language/format order; automatic captions are the fallback. Supported tracks have `ext: srv1` and HTTP/HTTPS URLs. Track request headers take precedence over video request headers. A missing title, mismatched video ID, unavailable XML track, malformed XML, or empty transcript produces an error.

An uncached run executes `yt-dlp` from `PATH` to extract metadata for `https://www.youtube.com/watch?v=<ID>`. It disables user configuration, plugins, yt-dlp's cache, and playlist processing; simulation prevents media downloads. A 30-second deadline covers extraction and caption download together. YouTube's access restrictions and yt-dlp's external runtime requirements still apply.

Cached objects are stored indefinitely at `$XDG_CACHE_HOME/youtube-captions-dl/<ID>.v9.json`, or `~/.cache/youtube-captions-dl/<ID>.v9.json` when XDG is empty/unset. A nonempty XDG path must be absolute. Cache hits skip yt-dlp and HTTP. New entries are staged in the same directory, synced, and atomically replaced; Unix files have mode 0600. Cache read/write failures produce stderr warnings and retrieval continues when possible. Remove a video's cache file to fetch again. The Go `.v8.txt` cache is a different format.

XML entities and encoded HTML markup are decoded into plain text. Ordinary cues join with spaces. Speaker cues beginning `>>` and bracketed sound cues begin separate lines. Human output prints the title, a terminal-width divider (80 columns without a detected width), and text wrapped to 80 display columns without splitting long words. Agent JSON contains `video_id`, `title`, `language`, `generated` (boolean), and `text` (unwrapped cue groups); one newline terminates the object.

## Environment and results

| Variable | Behavior |
| :--- | :--- |
| `AGENT` | `1`, `true`, and `yes` select compact JSON output; unset/`0` select human output. |
| `XDG_CACHE_HOME` | Absolute cache parent; empty/unset uses the resolved home directory's `.cache`. |
| `PATH` | Lookup path for yt-dlp and any runtimes it invokes. |
| `COLUMNS` | Shared terminal-width override for human help and dividers. |
| `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY` | Standard proxy settings used by the HTTP client and yt-dlp according to each tool's policy. |

Exit 0 means success or displayed metadata/help. Exit 1 means argument, cache-location, dependency, extraction, HTTP, parsing, or output failure. Exit 130 means cancellation. Errors are printed once on stderr (`ERR:` in agent mode, `[ERROR]` in human mode); cache warnings use `WARN:` or `[WARN]`. Runtime failures omit usage. A failed output write can occur after a cache entry has been published. Home paths in diagnostics are displayed with `~`.

## Examples

```sh
AGENT=1 youtube-captions-dl caption get https://youtu.be/tG1CSRaJhKQ
youtube-captions-dl caption get tG1CSRaJhKQ
youtube-captions-dl --version
youtube-captions-dl help caption get
youtube-captions-dl skill
```
