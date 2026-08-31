//! The confirmation gate for `unf prune`.
//!
//! `prune` is the only command in UNF that destroys recorded history with no
//! undo. `restore` can prompt loosely — when it cannot ask, it proceeds, and
//! that is safe only because it writes a safety snapshot first. `prune` has no
//! such fallback: there is no trash directory and no pre-prune snapshot.
//!
//! The gate reads the prune's actual impact, not the shape of the command. A
//! prune that trims old snapshots and leaves every project with history runs
//! exactly as it always has, terminal or not. A prune that would leave a
//! project with zero snapshots — its whole recording gone — is the shape of
//! the incident this exists to stop: an `--all-projects` run against a live
//! daemon by an agent that could not be prompted erased 155,783 snapshots
//! across 14 projects, eleven of them emptied outright.
//!
//! The decision is a pure function so both the terminal and the non-terminal
//! branch are unit-testable. [`confirm`] is the only part that touches I/O.

use std::io::{self, Write};

use crate::cli::OutputFormat;
use crate::error::UnfError;

/// What a prune invocation must do before it is allowed to delete anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruneGate {
    /// Delete now. The caller already has consent, or nothing irreplaceable
    /// is at stake.
    Proceed,
    /// Report what would go, delete nothing, exit 0.
    DryRunOnly,
    /// Report what would go, then ask on the terminal.
    Prompt,
    /// Refuse. The run would erase a project outright and nobody can answer a
    /// prompt.
    RefuseNoTty,
}

/// Decides how a prune invocation must be gated.
///
/// Pure: every input arrives as a value, including whether stdout is a
/// terminal and whether the prune is catastrophic, so every branch is
/// testable without a real TTY or a database.
///
/// `would_empty_any_project` is the whole safety judgement: it is `true` only
/// when this prune would leave at least one project with zero snapshots. The
/// caller computes it from the same dry-run pass that renders the preview.
///
/// The rules, in order:
/// 1. `--dry-run` deletes nothing, whatever else is set.
/// 2. A single-project prune is unchanged. Its blast radius is the one project
///    the user is standing in, so it earns no new friction.
/// 3. `--yes` is explicit consent.
/// 4. A prune that leaves every project with history is routine. It proceeds
///    unattended, in any format — this is what keeps cron jobs working.
/// 5. Otherwise a project loses everything, so a terminal gets asked.
/// 6. Nothing left to ask: refuse rather than guess.
pub fn decide(
    all_projects: bool,
    yes: bool,
    dry_run: bool,
    is_tty: bool,
    format: OutputFormat,
    would_empty_any_project: bool,
) -> PruneGate {
    if dry_run {
        return PruneGate::DryRunOnly;
    }
    if !all_projects || yes || !would_empty_any_project {
        return PruneGate::Proceed;
    }
    if is_tty && format != OutputFormat::Json {
        return PruneGate::Prompt;
    }
    PruneGate::RefuseNoTty
}

/// States what the prune would erase, naming every project it would empty.
///
/// Shared by the refusal and the prompt so a script and a human are told the
/// identical thing. Projects are named by their registered root path — the
/// same string the preview lists them under.
fn what_it_would_erase(emptied: &[String]) -> String {
    format!(
        "this prune would erase the entire recorded history of {} project{}: {}",
        emptied.len(),
        if emptied.len() == 1 { "" } else { "s" },
        emptied.join(", ")
    )
}

/// The error for a prune that would empty a project with no way to ask.
///
/// Names both escape hatches, because the caller that hits this is usually a
/// script or an agent that can add a flag but cannot answer a question.
pub fn refusal(emptied: &[String]) -> UnfError {
    UnfError::InvalidArgument(format!(
        "{}. Prune writes no safety snapshot and there is no trash directory, \
         so this cannot be undone. Re-run with --dry-run to preview it, or \
         --yes to confirm it. Prunes that leave history behind are unaffected.",
        what_it_would_erase(emptied)
    ))
}

/// The warning shown above the confirmation prompt.
///
/// The preview already lists each project's `keep nothing` line; this says the
/// same thing once, in one place, so the stake is not something you have to
/// reconstruct from a long list.
pub fn prompt_warning(emptied: &[String]) -> String {
    format!("\nWarning: {}.\n", what_it_would_erase(emptied))
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

    const CATASTROPHIC: bool = true;
    const ROUTINE: bool = false;

    /// THE REGRESSION TEST FOR THE INCIDENT.
    ///
    /// An agent ran `unf prune --all-projects` with no terminal attached and
    /// no `--yes`, against a cutoff that took everything. Nothing asked,
    /// nothing warned, and 155,783 snapshots across 14 real projects were gone
    /// with no undo. That combination must now refuse and exit non-zero.
    #[test]
    fn catastrophic_prune_without_tty_and_without_yes_refuses() {
        assert_eq!(
            decide(true, false, false, false, OutputFormat::Human, CATASTROPHIC),
            PruneGate::RefuseNoTty,
            "a non-interactive prune that empties a project must refuse"
        );
    }

    /// THE TEST THAT PROVES CRON JOBS STILL WORK.
    ///
    /// `unf prune --all-projects --older-than 30d` from a cron job or a CI
    /// step has no terminal and no `--yes`. As long as it leaves every project
    /// with history, it is the routine housekeeping this tool has always done,
    /// and it must run unattended exactly as before. Gating on the command
    /// shape rather than its impact would have broken every one of these.
    #[test]
    fn routine_prune_without_tty_and_without_yes_proceeds() {
        for &format in &[OutputFormat::Human, OutputFormat::Json] {
            assert_eq!(
                decide(true, false, false, false, format, ROUTINE),
                PruneGate::Proceed,
                "an unattended --all-projects prune that keeps history must not be gated"
            );
        }
    }

    /// The refusal must name `--yes`, or the caller has no way forward, and it
    /// must name the projects, or the caller cannot tell what was at stake.
    #[test]
    fn refusal_names_the_escape_hatches_and_the_projects() {
        let msg = refusal(&["/p/one".to_string(), "/p/two".to_string()]).to_string();
        assert!(msg.contains("--yes"), "refusal must name --yes: {}", msg);
        assert!(
            msg.contains("--dry-run"),
            "refusal must name --dry-run: {}",
            msg
        );
        assert!(
            msg.contains("/p/one"),
            "refusal must name projects: {}",
            msg
        );
        assert!(
            msg.contains("/p/two"),
            "refusal must name projects: {}",
            msg
        );
        assert!(
            msg.contains("2 projects"),
            "refusal must count the projects: {}",
            msg
        );
    }

    /// One project is "1 project", not "1 projects".
    #[test]
    fn refusal_is_singular_for_one_project() {
        let msg = refusal(&["/p/only".to_string()]).to_string();
        assert!(msg.contains("1 project:"), "{}", msg);
    }

    /// The prompt says the same thing as the refusal before it asks, so a
    /// human and a script learn the identical fact.
    #[test]
    fn prompt_warning_names_the_same_projects_as_the_refusal() {
        let emptied = vec!["/p/one".to_string()];
        let warning = prompt_warning(&emptied);
        assert!(warning.contains("/p/one"), "{}", warning);
        assert!(
            warning.contains("would erase the entire recorded history"),
            "{}",
            warning
        );
    }

    /// The refusal must exit non-zero. Exit 0 would let a script march on as
    /// if the prune had happened.
    #[test]
    fn refusal_exits_non_zero() {
        let code = crate::error::ExitCode::from(&refusal(&["/p/one".to_string()])) as i32;
        assert_ne!(code, 0, "refusing to prune must not look like success");
    }

    #[test]
    fn catastrophic_prune_on_a_terminal_prompts() {
        assert_eq!(
            decide(true, false, false, true, OutputFormat::Human, CATASTROPHIC),
            PruneGate::Prompt
        );
    }

    #[test]
    fn yes_proceeds_even_when_catastrophic() {
        for &is_tty in &[true, false] {
            for &format in &[OutputFormat::Human, OutputFormat::Json] {
                assert_eq!(
                    decide(true, true, false, is_tty, format, CATASTROPHIC),
                    PruneGate::Proceed,
                    "--yes is consent (tty={})",
                    is_tty
                );
            }
        }
    }

    /// `--dry-run` wins over every other input, `--yes` and catastrophe
    /// included.
    #[test]
    fn dry_run_never_deletes_regardless_of_tty() {
        for &all_projects in &[true, false] {
            for &yes in &[true, false] {
                for &is_tty in &[true, false] {
                    for &format in &[OutputFormat::Human, OutputFormat::Json] {
                        for &empties in &[CATASTROPHIC, ROUTINE] {
                            assert_eq!(
                                decide(all_projects, yes, true, is_tty, format, empties),
                                PruneGate::DryRunOnly,
                                "--dry-run must never delete (all={}, yes={}, tty={}, empties={})",
                                all_projects,
                                yes,
                                is_tty,
                                empties
                            );
                        }
                    }
                }
            }
        }
    }

    /// JSON has no channel for a prompt, so a catastrophic prune refuses
    /// there even when a terminal happens to be attached. A routine one is
    /// untouched — see `routine_prune_without_tty_and_without_yes_proceeds`.
    #[test]
    fn json_refuses_a_catastrophic_prune_even_on_a_terminal() {
        assert_eq!(
            decide(true, false, false, true, OutputFormat::Json, CATASTROPHIC),
            PruneGate::RefuseNoTty
        );
        assert_eq!(
            decide(true, false, false, false, OutputFormat::Json, CATASTROPHIC),
            PruneGate::RefuseNoTty
        );
    }

    /// Single-project prune keeps its old behaviour in every combination: it
    /// either proceeds or, with `--dry-run`, previews. It never prompts and it
    /// never refuses, catastrophic or not.
    #[test]
    fn single_project_is_never_gated() {
        for &yes in &[true, false] {
            for &is_tty in &[true, false] {
                for &format in &[OutputFormat::Human, OutputFormat::Json] {
                    for &empties in &[CATASTROPHIC, ROUTINE] {
                        assert_eq!(
                            decide(false, yes, false, is_tty, format, empties),
                            PruneGate::Proceed,
                            "single-project prune must not gain friction (yes={}, tty={})",
                            yes,
                            is_tty
                        );
                        assert_eq!(
                            decide(false, yes, true, is_tty, format, empties),
                            PruneGate::DryRunOnly
                        );
                    }
                }
            }
        }
    }
}
