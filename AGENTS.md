---
created_on: 2026-10-14 16:47
last_modified: 2026-10-14 16:59
status: current
---

# youtube-captions-dl

Independent Rust CLI for title/caption retrieval. Parent [AGENTS.md](../AGENTS.md) applies.

## Commands

- `just run caption get <VIDEO>` and `just run-ai caption get <VIDEO>`.
- `just test`, `just lint`, `just coverage`, and `just check`.
- `just build` produces `bin/youtube-captions-dl`; Cargo's intermediate files stay in `target/`.

## Contracts

- Integrate native yt-dlp metadata and subtitle request headers; use the shared `cli-deps::ProcessRunner`, without manual process emulation or private InnerTube request copies.
- Preserve the single 30-second workflow deadline and typed cancellation. Cache hits must avoid HTTP and subprocess execution.
- Keep manual-track priority, extractor JSON order, and `srv1` XML validation. Quick XML's deserializer alone does not enforce the expected root or a single document; retain explicit native Reader validation.
- Retain atomic same-directory cache replacement and private Unix file permissions. Cache payloads hold cues rather than human presentation, so formatting is applied independently in both modes.
- Only the CLI error boundary reports returned errors. Route diagnostic paths through `cli_command_kit::display_message`; filesystem calls keep native paths.
- Keep `skills/youtube-captions-dl/SKILL.md` synchronized in the same change as commands, arguments, flags, aliases, defaults, environment, output, and side effects; update its `last_modified` metadata. The `skill` pointer names the verified Go source repository; it does not prove that Rust artifacts or this skill have been published there.
- Whenever runtime results change, change a corresponding behavioral test and retain at least 90% line coverage; `scripts/` is excluded. Prove meaningful regressions by disabling and restoring the implementation.
- Use local HTTP servers and subprocesses for tests. Keep temporary files and evidence in `.tmp/`; scope rustfmt to this package's explicit files.
- Never claim live YouTube, macOS, Windows, release, or authenticated behavior from local fixtures. Never publish releases, tags, packages, or deployments without explicit authorization.
- Persist standing instructions in place; keep progress and test results outside this file.

## References

- [Dependency choices](DEPENDENCIES.md).
- [Usage skill](skills/youtube-captions-dl/SKILL.md).
