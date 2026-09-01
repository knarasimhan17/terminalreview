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
use crate::git::{PreparedReview, Repository};
use crate::persistence::{list_revisions, persist_revision};
use crate::tui::{CommitPickerOutcome, ReviewOutcome, ReviewTarget};

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
        None => run_review(
            &repository,
            cli.revset.as_deref(),
            cli.working_tree,
            cli.stdout,
        ),
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

fn run_review(
    repository: &Repository,
    revset: Option<&str>,
    working_tree: bool,
    stdout: bool,
) -> Result<()> {
    let thread = repository.current_thread()?;
    if revset.is_some() || working_tree {
        let prepared = repository.prepare_review(revset)?;
        let diff = ParsedDiff::parse(&prepared.diff);
        let outcome = tui::run(diff)?;
        return finish_review(repository, &thread, prepared, outcome, stdout);
    }

    let commits = repository.recent_commits()?;
    let working_tree = repository.prepare_review(None)?;
    let working_tree_clean = working_tree.diff.is_empty();
    let outcome = tui::run_picker(commits, working_tree_clean, move |target| match target {
        ReviewTarget::WorkingTree => Ok(working_tree),
        ReviewTarget::CommitRange {
            base_sha,
            source_sha,
        } => {
            let revset = format!("{base_sha}..{source_sha}");
            repository.prepare_review(Some(&revset))
        }
    })?;

    match outcome {
        CommitPickerOutcome::Reviewed { prepared, outcome } => {
            finish_review(repository, &thread, prepared, outcome, stdout)
        }
        CommitPickerOutcome::NoChanges => {
            println!("No changes to review.");
            Ok(())
        }
        CommitPickerOutcome::Quit => Ok(()),
    }
}

fn finish_review(
    repository: &Repository,
    thread: &str,
    prepared: PreparedReview,
    outcome: ReviewOutcome,
    stdout: bool,
) -> Result<()> {
    let ReviewOutcome::Export(comments) = outcome else {
        return Ok(());
    };

    let revision = persist_revision(repository, thread, &prepared, comments)?;
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
