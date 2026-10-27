use crate::{Cache, Client, FetchError, format_cues, parse_video_id};
use anyhow::Result;
use cli_command_kit::{
    HelpOptions, Mode,
    clap::{Arg, ArgAction, Command},
};
use std::{ffi::OsString, io::Write};
use tokio_util::sync::CancellationToken;

const SKILL: &str = "Install the youtube-captions-dl skill using the Skills CLI:\n\n  npx skills add alexgorbatchev/youtube-captions-dl --skill youtube-captions-dl\n\nSkill:\n\n  https://github.com/alexgorbatchev/youtube-captions-dl/tree/main/skills/youtube-captions-dl\n";

pub struct Runtime {
    pub cache: Cache,
    pub client: Client,
}

pub fn command() -> Command {
    Command::new("youtube-captions-dl")
        .about("Download a YouTube title and plain-text captions")
        .arg_required_else_help(true)
        .arg(
            Arg::new("version")
                .long("version")
                .short('v')
                .action(ArgAction::SetTrue)
                .exclusive(true)
                .help("Print the raw version"),
        )
        .subcommand(
            Command::new("caption")
                .about("Read video captions")
                .subcommand_required(true)
                .subcommand(
                    Command::new("get")
                        .about("Download captions or use their cached copy")
                        .arg(
                            Arg::new("VIDEO")
                                .required(true)
                                .help("YouTube URL or11-character video ID")
                                .value_parser(|value: &str| {
                                    parse_video_id(value).map_err(|error| error.to_string())
                                }),
                        ),
                ),
        )
}

pub fn help_options() -> HelpOptions {
    HelpOptions {
        skill: Some(SKILL.into()),
        ..Default::default()
    }
}

async fn perform(
    matches: &cli_command_kit::clap::ArgMatches,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
    mode: Mode,
    cancel: &CancellationToken,
    runtime: Option<&Runtime>,
) -> Result<()> {
    if cli_command_kit::write_skill(matches, &help_options(), output)? {
        return Ok(());
    }
    if matches.get_flag("version") {
        writeln!(output, "{}", env!("CARGO_PKG_VERSION"))?;
        return Ok(());
    }
    let arguments = matches
        .subcommand_matches("caption")
        .unwrap()
        .subcommand_matches("get")
        .unwrap();
    let video_id = arguments.get_one::<String>("VIDEO").unwrap();
    let owned;
    let runtime = match runtime {
        Some(runtime) => runtime,
        None => {
            owned = Runtime {
                cache: Cache::new()?,
                client: Client::new()?,
            };
            &owned
        }
    };
    if cancel.is_cancelled() {
        return Err(FetchError::Cancelled.into());
    }
    let cached = match runtime.cache.load(video_id) {
        Ok(cached) => cached,
        Err(error) => {
            writeln!(
                diagnostics,
                "{} reading cache failed: {}",
                if mode == Mode::Agent {
                    "WARN:"
                } else {
                    "[WARN]"
                },
                cli_command_kit::display_message(&format!("{error:#}"))
            )?;
            None
        }
    };
    let captions = if let Some(cached) = cached {
        cached
    } else {
        let captions = runtime.client.fetch(video_id, cancel).await?;
        if cancel.is_cancelled() {
            return Err(FetchError::Cancelled.into());
        }
        if let Err(error) = runtime.cache.save(&captions) {
            writeln!(
                diagnostics,
                "{} writing cache failed: {}",
                if mode == Mode::Agent {
                    "WARN:"
                } else {
                    "[WARN]"
                },
                cli_command_kit::display_message(&format!("{error:#}"))
            )?;
        }
        captions
    };
    if mode == Mode::Agent {
        serde_json::to_writer(
            &mut *output,
            &serde_json::json!({"video_id":captions.video_id,"title":captions.title,"language":captions.language,"generated":captions.generated,"text":format_cues(&captions.cues,None)}),
        )?;
        writeln!(output)?;
    } else {
        writeln!(
            output,
            "{}\n{}\n{}",
            captions.title,
            "-".repeat(match cli_command_kit::terminal_width() {
                0 => 80,
                width => width,
            }),
            format_cues(&captions.cues, Some(80))
        )?;
    }
    Ok(())
}

pub async fn entry(
    args: impl IntoIterator<Item = OsString>,
    output: &mut impl Write,
    diagnostics: &mut impl Write,
    mode: Mode,
    cancel: &CancellationToken,
    runtime: Option<&Runtime>,
) -> u8 {
    let root = cli_command_kit::setup(command(), mode, &help_options())
        .expect("valid static command tree");
    let result = match root.try_get_matches_from(args) {
        Ok(matches) => perform(&matches, output, diagnostics, mode, cancel, runtime).await,
        Err(error) if !error.use_stderr() => output
            .write_all(error.to_string().as_bytes())
            .map_err(Into::into),
        Err(error) => {
            let _ = write!(
                diagnostics,
                "{}",
                cli_command_kit::display_message(&error.to_string())
            );
            return 1;
        }
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(
                diagnostics,
                "{} {}",
                if mode == Mode::Agent {
                    "ERR:"
                } else {
                    "[ERROR]"
                },
                cli_command_kit::display_message(&format!("{error:#}"))
            );
            if matches!(
                error.downcast_ref::<FetchError>(),
                Some(FetchError::Cancelled)
            ) || matches!(
                error.downcast_ref::<cli_deps::Error>(),
                Some(cli_deps::Error::Cancelled)
            ) {
                130
            } else {
                1
            }
        }
    }
}
