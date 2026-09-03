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

    #[arg(short = 'w', long, conflicts_with = "revset")]
    pub(crate) working_tree: bool,

    #[arg(long)]
    pub(crate) stdout: bool,

    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Revs,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReviewLaunch {
    Direct { revset: Option<String> },
    Picker,
}

pub(crate) fn review_launch(
    revset: Option<&str>,
    working_tree: bool,
    has_uncommitted: bool,
) -> ReviewLaunch {
    if let Some(revset) = revset {
        ReviewLaunch::Direct {
            revset: Some(revset.to_owned()),
        }
    } else if working_tree || has_uncommitted {
        ReviewLaunch::Direct { revset: None }
    } else {
        ReviewLaunch::Picker
    }
}

#[cfg(test)]
mod tests {
    use super::{ReviewLaunch, review_launch};

    #[test]
    fn dirty_working_tree_opens_uncommitted_changes() {
        assert_eq!(
            review_launch(None, false, true),
            ReviewLaunch::Direct { revset: None },
            "uncommitted changes must skip the commit picker"
        );
    }

    #[test]
    fn clean_working_tree_opens_the_commit_picker() {
        assert_eq!(
            review_launch(None, false, false),
            ReviewLaunch::Picker,
            "a clean tree must let the user choose commits"
        );
    }

    #[test]
    fn working_tree_flag_skips_the_picker_even_when_clean() {
        assert_eq!(
            review_launch(None, true, false),
            ReviewLaunch::Direct { revset: None },
            "-w must still review the working tree directly"
        );
    }

    #[test]
    fn explicit_revset_wins_over_uncommitted_changes() {
        assert_eq!(
            review_launch(Some("HEAD~1"), false, true),
            ReviewLaunch::Direct {
                revset: Some("HEAD~1".to_owned()),
            },
            "-r must review the requested range even when the working tree is dirty"
        );
    }
}
