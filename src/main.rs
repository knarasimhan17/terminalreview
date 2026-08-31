mod cli;
mod diff;
mod export;
mod git;
mod model;
mod persistence;
mod tui;

use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;

use crate::cli::{Cli, Command};
use crate::diff::ParsedDiff;
use crate::export::{copy_to_clipboard, format_comments};
use crate::git::Repository;
use crate::persistence::{list_revisions, persist_revision};
use crate::tui::ReviewOutcome;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("trv: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let current_dir = env::current_dir().context("current directory is unavailable")?;
    let repository = Repository::discover(&current_dir)?;

    match cli.command {
        Some(Command::Revs) => print_revisions(&repository),
        None => run_review(&repository, cli.revset.as_deref(), cli.stdout),
    }
}

fn print_revisions(repository: &Repository) -> Result<()> {
    let thread = repository.current_thread()?;
    for revision in list_revisions(repository.git_dir(), &thread)? {
        println!(
            "{}\t{}\t{}\t{}",
            revision.rev,
            revision.timestamp,
            revision.comments.len(),
            revision.base_commit_sha
        );
    }
    Ok(())
}

fn run_review(repository: &Repository, revset: Option<&str>, stdout: bool) -> Result<()> {
    let thread = repository.current_thread()?;
    let prepared = repository.prepare_review(revset)?;
    let diff = ParsedDiff::parse(&prepared.diff);

    let ReviewOutcome::Export(comments) = tui::run(diff)? else {
        return Ok(());
    };

    let revision = persist_revision(repository, &thread, &prepared, comments)?;
    let formatted = format_comments(&revision.comments);

    if stdout {
        if !formatted.is_empty() {
            let mut output = io::stdout().lock();
            writeln!(output, "{formatted}").context("failed to write exported comments")?;
        }
        eprintln!("saved rev-{}", revision.rev);
    } else {
        let method = copy_to_clipboard(&formatted)?;
        eprintln!("saved rev-{}; copied comments via {method}", revision.rev);
    }

    Ok(())
}
