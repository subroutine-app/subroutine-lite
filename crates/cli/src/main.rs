mod args;
mod backend;
mod commands;
mod create;
mod input;
mod resources;

use std::{
    io::{self, Write},
    process::ExitCode,
    time::Duration,
};

use anyhow::Result;
use chrono::Local;
use clap::Parser;
use serde_json::json;

use args::{Cli, Command};
use backend::Backend;

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error)
            if error.chain().any(|cause| {
                cause
                    .downcast_ref::<io::Error>()
                    .is_some_and(|error| error.kind() == io::ErrorKind::BrokenPipe)
            }) =>
        {
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    let now = cli
        .now
        .map(|time| time.with_timezone(&Local))
        .unwrap_or_else(Local::now);
    if let Command::Parse(args) = &cli.command {
        let value = create::parsed_value(args.kind, &args.text, now)?;
        return output(&value, cli.pretty, cli.quiet);
    }
    let input = cli.command.input_path().map(input::json).transpose()?;
    let (backend, data) = Backend::open(
        cli.db.as_deref(),
        cli.server.as_deref(),
        cli.token_file.as_deref(),
        Duration::from_secs(cli.timeout),
        cli.dry_run,
    )
    .await?;
    let plan = commands::execute(cli.command, &data, now, input)?;
    let value = if cli.dry_run {
        json!({ "dry_run": true, "result": plan.output, "changes": plan.patch })
    } else {
        plan.output
    };
    let bytes = encode(&value, cli.pretty)?;
    if !cli.dry_run {
        backend.commit(&data, plan.patch).await?;
    }
    if !cli.quiet {
        let mut stdout = io::stdout().lock();
        stdout.write_all(&bytes)?;
        stdout.flush()?;
    }
    Ok(())
}

fn encode(value: &serde_json::Value, pretty: bool) -> Result<Vec<u8>> {
    let mut bytes = if pretty {
        serde_json::to_vec_pretty(value)?
    } else {
        serde_json::to_vec(value)?
    };
    bytes.push(b'\n');
    Ok(bytes)
}

fn output(value: &serde_json::Value, pretty: bool, quiet: bool) -> Result<()> {
    if !quiet {
        let mut stdout = io::stdout().lock();
        stdout.write_all(&encode(value, pretty)?)?;
        stdout.flush()?;
    }
    Ok(())
}
