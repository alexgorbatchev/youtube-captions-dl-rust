---
created_on: 2026-10-14 16:47
last_modified: 2026-10-14 16:59
status: current
---

# Dependencies and source behavior

The Go source at `../../youtube-captions-dl` (MIT, Alex Gorbatchev) has no third-party Go dependencies. It manually extracts a private InnerTube key, requests an Android player response, selects a caption track, decodes XML, and caches a formatted transcript. The Rust project preserves URL forms, manual-caption preference, cue grouping, offline caching, and failure visibility with a maintained extractor and native Rust libraries.

| Requirement | Native replacement | Grounding |
| :--- | :--- | :--- |
| YouTube metadata and caption discovery | External [yt-dlp](https://github.com/yt-dlp/yt-dlp), through `cli-deps::ProcessRunner` | Installed 2026.08.19 help and [native source](https://github.com/yt-dlp/yt-dlp/blob/2026.08.19/yt_dlp/YoutubeDL.py), including subtitle `http_headers` fallback. No downloaded media, user configuration, plugins, or extractor cache. |
| HTTP download/deadline | reqwest 0.13.5, Tokio 1, tokio-util 0.7 | Native request/header/status/body and cancellation APIs; local servers exercise the contract. |
| XML/plain text | quick-xml 0.42 with serde, scraper 0.27, html-escape 0.2 | Native Reader enforces document structure, serde decodes cues, HTML parsing strips encoded markup. |
| Wrapping | textwrap 0.16 | Native Unicode display widths and word wrapping. |
| Cache | dirs 6, tempfile 3, serde/serde_json 1 | XDG/home lookup, atomic private same-directory files; `preserve_order` retains provider JSON order. |
| CLI | cli-command-kit, Clap via its re-export, anyhow/thiserror | Shared help, skill pointer, path formatting, native typed failures. |

The researched MIT crates `yt-transcript-rs 0.1.8`, `ytsrt 0.1.2`, and `ytt 1.1.0` do not satisfy this tool's selection/title/URL/client contract directly. Inspected sources show language-only re-selection and unordered track maps, first-track-only retrieval, or broad CLI/ChatGPT dependencies. Adopting one would require bypassing its native API or retaining a private endpoint implementation. The maintained yt-dlp extractor is used directly instead, adding an explicit runtime prerequisite.

[YouTube's official captions download API](https://developers.google.com/youtube/v3/docs/captions/download) requires authorized editing access and does not provide anonymous public-video transcript retrieval. yt-dlp extraction remains subject to YouTube endpoint, access, and JavaScript challenge changes. Live retrieval is separate from local verification.

## Observable corrections and differences

- URL host normalization accepts uppercase `WWW` and rejects lookalike hosts and non-HTTP schemes.
- Human wrapping uses Unicode display columns, and malformed/multiple-root XML is rejected rather than partially accepted.
- Both output modes propagate write failures; cache warnings remain nonfatal when they can be reported.
- `caption get <VIDEO>` is the subject-first command; agent mode emits JSON rather than a human divider. No legacy argument compatibility layer is provided.
- `.v9.json` stores validated cue objects; Go `.v8.txt` entries remain untouched. Cache lifetime remains indefinite.
- Video and per-track native request headers are preserved. No login/cookie configuration is inferred from the user's yt-dlp settings, because extraction disables user configuration.

Direct Rust dependencies use MIT, Apache-2.0, or compatible permissive licenses; scraper is ISC. The separately installed yt-dlp release retains its own licensing and runtime requirements. Consult `Cargo.lock` for exact resolved versions and upstream packages for full notices. No upstream extractor source is copied into the project.
