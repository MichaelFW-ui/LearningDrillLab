mod ai;
mod app;
mod commands;
mod domain;
mod sandbox;
mod skills;

#[tauri::command]
fn startup_status() -> &'static str {
    "Tauri 2 已启动"
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(commands::ManagedState(
            std::sync::Mutex::new(app::AppState::load()),
            std::sync::Arc::new(std::sync::Mutex::new(None)),
        ))
        .invoke_handler(tauri::generate_handler![
            startup_status,
            commands::get_state,
            commands::get_storage_path,
            commands::save_settings,
            commands::fetch_models,
            commands::new_topic,
            commands::switch_topic,
            commands::delete_topic,
            commands::rename_topic,
            commands::set_topic_sort,
            commands::select_exercise,
            commands::generate,
            commands::follow_up,
            commands::submit_answer,
            commands::regenerate_exercises,
            commands::request_experiment,
            commands::execute_experiment,
            commands::list_skills,
            commands::cancel_task,
        ])
        .run(tauri::generate_context!())
        .expect("无法启动 Learning Drill Lab");
}
