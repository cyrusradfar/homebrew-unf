use std::sync::Mutex;

use crate::error::AppError;

/// Minimal application state: only the currently selected project path.
pub struct AppState {
    pub selected_project: Mutex<Option<String>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            selected_project: Mutex::new(None),
        }
    }

    /// Get the selected project path, or error if none selected.
    pub fn selected_project(&self) -> Result<String, AppError> {
        self.selected_project
            .lock()
            .map_err(|_| AppError::NoProject)?
            .clone()
            .ok_or(AppError::NoProject)
    }

    /// Set the selected project path.
    pub fn set_selected_project(&self, path: String) -> Result<(), AppError> {
        let mut guard = self
            .selected_project
            .lock()
            .map_err(|_| AppError::NoProject)?;
        *guard = Some(path);
        Ok(())
    }
}
