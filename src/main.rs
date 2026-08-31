mod cli;
mod diff;
mod git;
mod model;
mod persistence;

use std::collections::BTreeSet;
use std::env;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::Parser;

use crate::cli::{Cli, Command};
use crate::diff::{DiffLineKind, ParsedDiff};
use crate::git::Repository;
use crate::model::{Comment, Side};
use crate::persistence::{list_revisions, persist_revision};

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
    let comments = collect_comments(&diff)?;
    let revision = persist_revision(repository, &thread, &prepared, comments)?;
    let export_target = if stdout { "stdout" } else { "clipboard" };

    bail!(
        "comment export to {export_target} is not implemented after saving rev-{}",
        revision.rev
    )
}

fn collect_comments(diff: &ParsedDiff) -> Result<Vec<Comment>> {
    bail!(
        "review UI is not implemented; prepared {}",
        summarize_diff(diff)
    )
}

fn summarize_diff(diff: &ParsedDiff) -> String {
    let mut additions = 0;
    let mut deletions = 0;
    let mut numbered_lines = 0;
    let mut rendered_bytes = 0;
    let mut anchor_paths = BTreeSet::new();
    let mut old_anchors = 0;
    let mut new_anchors = 0;
    let mut highest_anchor_line = 0;

    for line in &diff.lines {
        rendered_bytes += line.text.len();
        match line.kind {
            DiffLineKind::Addition => additions += 1,
            DiffLineKind::Deletion => deletions += 1,
            DiffLineKind::File
            | DiffLineKind::Hunk
            | DiffLineKind::Context
            | DiffLineKind::Meta => {}
        }
        if line.old_line.is_some() || line.new_line.is_some() {
            numbered_lines += 1;
        }
        if let Some(anchor) = line.anchor() {
            anchor_paths.insert(anchor.path.as_str());
            highest_anchor_line = highest_anchor_line.max(anchor.line);
            match anchor.side {
                Side::Old => old_anchors += 1,
                Side::New => new_anchors += 1,
            }
        }
    }

    format!(
        "{} files, {} lines, {additions} additions, {deletions} deletions, \
         {numbered_lines} numbered lines, {} anchors across {} paths \
         ({old_anchors} old, {new_anchors} new, max line {highest_anchor_line}), \
         and {rendered_bytes} rendered bytes",
        diff.file_starts.len(),
        diff.lines.len(),
        old_anchors + new_anchors,
        anchor_paths.len()
    )
}
