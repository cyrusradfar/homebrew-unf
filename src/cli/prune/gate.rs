//! The confirmation gate for `unf prune`.
//!
//! `prune` is the only command in UNF that destroys recorded history with no
//! undo. `restore` can prompt loosely — when it cannot ask, it proceeds, and
//! that is safe only because it writes a safety snapshot first. `prune` has no
//! such fallback: there is no trash directory and no pre-prune snapshot, so a
//! non-interactive `--all-projects` run must refuse rather than guess. That
//! exact command, run against a live daemon by an agent that could not be
//! prompted, erased 155,783 snapshots across 14 projects.
//!
//! The decision is a pure function so both the terminal and the non-terminal
//! branch are unit-testable. [`confirm`] is the only part that touches I/O.

use std::io::{self, Write};

use crate::cli::OutputFormat;
use crate::error::UnfError;

/// What a prune invocation must do before it is allowed to delete anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruneGate {
    /// Delete now. The caller already has consent, or the blast radius is one
    /// project.
    Proceed,
    /// Report what would go, delete nothing, exit 0.
    DryRunOnly,
    /// Report what would go, then ask on the terminal.
    Prompt,
    /// Refuse. The run is destructive and wide, and nobody can answer a
    /// prompt.
    RefuseNoTty,
}

/// Decides how a prune invocation must be gated.
///
/// Pure: every input arrives as a value, including whether stdout is a
/// terminal, so both branches are testable without a real TTY.
///
/// The rules, in order:
/// 1. `--dry-run` deletes nothing, whatever else is set.
/// 2. A single-project prune is unchanged. Its blast radius is the one project
///    the user is standing in, so it earns no new friction.
/// 3. `--yes` is explicit consent.
/// 4. JSON mode has no channel for a prompt, so it refuses instead of asking.
/// 5. A terminal can be asked.
/// 6. What is left is a non-interactive `--all-projects` prune with no
///    consent. That is the incident this gate exists to stop.
pub fn decide(
    all_projects: bool,
    yes: bool,
    dry_run: bool,
    is_tty: bool,
    format: OutputFormat,
) -> PruneGate {
    if dry_run {
        return PruneGate::DryRunOnly;
    }
    if !all_projects || yes {
        return PruneGate::Proceed;
    }
    if format == OutputFormat::Json || !is_tty {
        return PruneGate::RefuseNoTty;
    }
    PruneGate::Prompt
}

/// The error for a wide prune that has no way to ask for consent.
///
/// Names both escape hatches, because the caller that hits this is usually a
/// script or an agent that can add a flag but cannot answer a question.
pub fn refusal() -> UnfError {
    UnfError::InvalidArgument(
        "refusing to prune every project without confirmation. \
         --all-projects permanently deletes snapshots from every registered \
         project and writes no safety snapshot, so there is no undo. \
         Re-run with --dry-run to preview it, or --yes to confirm it."
            .to_string(),
    )
}

/// Asks on the terminal. Anything but `y` cancels.
///
/// The side effect lives here so [`decide`] stays pure. Mirrors the prompt in
/// `cli::restore` down to the wording, so the two destructive commands read
/// the same way.
pub fn confirm() -> Result<(), UnfError> {
    print!("\nProceed? [y/N] ");
    io::stdout()
        .flush()
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to flush output: {}", e)))?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to read input: {}", e)))?;

    if input.trim().eq_ignore_ascii_case("y") {
        Ok(())
    } else {
        Err(UnfError::InvalidArgument("Prune cancelled.".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE REGRESSION TEST FOR THE INCIDENT.
    ///
    /// An agent ran `unf prune --all-projects` with no terminal attached and
    /// no `--yes`. Nothing asked, nothing warned, and 155,783 snapshots across
    /// 14 real projects were gone with no undo. That combination must now
    /// refuse and exit non-zero.
    #[test]
    fn all_projects_without_tty_and_without_yes_refuses() {
        assert_eq!(
            decide(true, false, false, false, OutputFormat::Human),
            PruneGate::RefuseNoTty,
            "a non-interactive --all-projects prune with no --yes must refuse"
        );
    }

    /// The refusal must name `--yes`, or the caller has no way forward.
    #[test]
    fn refusal_names_the_escape_hatches() {
        let msg = refusal().to_string();
        assert!(msg.contains("--yes"), "refusal must name --yes: {}", msg);
        assert!(
            msg.contains("--dry-run"),
            "refusal must name --dry-run: {}",
            msg
        );
    }

    /// The refusal must exit non-zero. Exit 0 would let a script march on as
    /// if the prune had happened.
    #[test]
    fn refusal_exits_non_zero() {
        let code = crate::error::ExitCode::from(&refusal()) as i32;
        assert_ne!(code, 0, "refusing to prune must not look like success");
    }

    #[test]
    fn all_projects_on_a_terminal_prompts() {
        assert_eq!(
            decide(true, false, false, true, OutputFormat::Human),
            PruneGate::Prompt
        );
    }

    #[test]
    fn yes_proceeds_with_or_without_a_terminal() {
        assert_eq!(
            decide(true, true, false, true, OutputFormat::Human),
            PruneGate::Proceed
        );
        assert_eq!(
            decide(true, true, false, false, OutputFormat::Human),
            PruneGate::Proceed
        );
        assert_eq!(
            decide(true, true, false, false, OutputFormat::Json),
            PruneGate::Proceed
        );
    }

    /// `--dry-run` wins over every other input, `--yes` included.
    #[test]
    fn dry_run_never_deletes_regardless_of_tty() {
        for &all_projects in &[true, false] {
            for &yes in &[true, false] {
                for &is_tty in &[true, false] {
                    for &format in &[OutputFormat::Human, OutputFormat::Json] {
                        assert_eq!(
                            decide(all_projects, yes, true, is_tty, format),
                            PruneGate::DryRunOnly,
                            "--dry-run must never delete (all_projects={}, yes={}, tty={})",
                            all_projects,
                            yes,
                            is_tty
                        );
                    }
                }
            }
        }
    }

    /// JSON mode cannot render a prompt or read an answer, so it refuses even
    /// when a terminal happens to be attached.
    #[test]
    fn json_without_yes_refuses_even_on_a_terminal() {
        assert_eq!(
            decide(true, false, false, true, OutputFormat::Json),
            PruneGate::RefuseNoTty
        );
        assert_eq!(
            decide(true, false, false, false, OutputFormat::Json),
            PruneGate::RefuseNoTty
        );
    }

    /// Single-project prune keeps its old behaviour in every combination: it
    /// either proceeds or, with `--dry-run`, previews. It never prompts and it
    /// never refuses.
    #[test]
    fn single_project_is_never_gated() {
        for &yes in &[true, false] {
            for &is_tty in &[true, false] {
                for &format in &[OutputFormat::Human, OutputFormat::Json] {
                    assert_eq!(
                        decide(false, yes, false, is_tty, format),
                        PruneGate::Proceed,
                        "single-project prune must not gain friction (yes={}, tty={})",
                        yes,
                        is_tty
                    );
                    assert_eq!(
                        decide(false, yes, true, is_tty, format),
                        PruneGate::DryRunOnly
                    );
                }
            }
        }
    }
}
