use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "trv",
    version,
    about = "Review Git changes in the terminal",
    args_conflicts_with_subcommands = true
)]
pub(crate) struct Cli {
    #[arg(short = 'r', long, value_name = "REVSET")]
    pub(crate) revset: Option<String>,

    #[arg(long)]
    pub(crate) stdout: bool,

    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Revs,
}
