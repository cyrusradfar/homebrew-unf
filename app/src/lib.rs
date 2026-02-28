mod commands;
mod error;
mod state;

use state::AppState;

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::project::list_projects,
            commands::project::select_project,
            commands::project::get_project_status,
            commands::project::remove_project,
            commands::history::get_log,
            commands::history::get_global_log,
            commands::history::get_density,
            commands::history::get_global_density,
            commands::diff::get_diff,
            commands::content::get_file_content,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
