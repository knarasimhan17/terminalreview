mod cli;
mod diff;
mod export;
mod git;
mod model;
mod persistence;
mod session;
mod tui;

use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;

use crate::cli::{Cli, Command, ReviewLaunch, review_launch};
use crate::export::{copy_to_clipboard, format_comments};
use crate::git::{PreparedReview, Repository};
use crate::persistence::{list_revisions, persist_revision};
use crate::session::ReviewSession;
use crate::tui::{CommitPickerOutcome, ReviewOutcome};

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
    match review_launch(revset, working_tree, repository.has_uncommitted_changes()?) {
        ReviewLaunch::Direct { revset } => {
            let prepared = repository.prepare_review(revset.as_deref())?;
            let session = open_review_session(repository, &thread, &prepared)?;
            let outcome = tui::run(session)?;
            finish_review(repository, &thread, prepared, outcome, stdout)
        }
        ReviewLaunch::Picker => {
            let commits = repository.recent_commits()?;
            let outcome = tui::run_picker(
                commits,
                |target| {
                    let revset = format!("{}..{}", target.base_sha, target.source_sha);
                    repository.prepare_review(Some(&revset))
                },
                |prepared| open_review_session(repository, &thread, prepared),
            )?;

            match outcome {
                CommitPickerOutcome::Reviewed { prepared, outcome } => {
                    finish_review(repository, &thread, prepared, outcome, stdout)
                }
                CommitPickerOutcome::Quit => Ok(()),
            }
        }
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

fn open_review_session(
    repository: &Repository,
    thread: &str,
    prepared: &PreparedReview,
) -> Result<ReviewSession> {
    let revisions = list_revisions(repository.git_dir(), thread)?;
    ReviewSession::open(repository, prepared, &revisions)
}
