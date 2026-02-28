//! `unf __boot` hidden subcommand implementation.
//!
//! DEPRECATED: Boot initialization is now handled by the sentinel.
//! This command is kept for backwards compatibility only.

use crate::error::UnfError;

/// Runs the `unf __boot` command.
///
/// Deprecated. The sentinel now handles boot initialization directly.
pub fn run() -> Result<(), UnfError> {
    eprintln!("Note: __boot is deprecated. The sentinel now handles boot initialization.");
    eprintln!("Run 'unf watch' to update your LaunchAgent configuration.");
    Ok(())
}

#[cfg(test)]
mod tests {
    // Boot command tests are integration-level since they involve
    // spawning processes and modifying the filesystem.
    // Unit tests for the registry and process modules cover the
    // underlying logic.
}
