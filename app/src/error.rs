/// Application error type for Tauri commands.
///
/// All errors are serialized as strings for IPC transport.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Failed to start unf: {0}")]
    SpawnFailed(String),

    #[error("unf error: {0}")]
    UnfError(String),

    #[error("Failed to parse unf output: {0}")]
    ParseError(String),

    #[error("No project selected")]
    NoProject,

    #[error("Task failed: {0}")]
    JoinFailed(String),
}

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
