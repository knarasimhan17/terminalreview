mod cli;
#[cfg(test)]
mod model;

use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Revs) => not_implemented("revs command"),
        None => review_not_implemented(cli.revset.as_deref(), cli.stdout),
    }
}

fn review_not_implemented(_revset: Option<&str>, _stdout: bool) -> ExitCode {
    not_implemented("review command")
}

fn not_implemented(command: &str) -> ExitCode {
    eprintln!("trv: {command} is not implemented");
    ExitCode::FAILURE
}
