
pub mod brightwheel;

use std::sync::Mutex;

use tauri::{Builder, Manager, State};

struct AppState {
  brightwheel: brightwheel::Brightwheel,
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn login(state_mutex: State<'_, Mutex<AppState>>, email: &str, password: &str) -> String {
    let state = state_mutex.lock().unwrap();
    state.brightwheel.authenticate_email_password(email, password);
    format!("From Rust: submitted login for {}", email)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    Builder::default()
        .setup(|app| {
            app.manage(Mutex::new(AppState {
                brightwheel: brightwheel::Brightwheel::new(),
            }));
            Ok(())            
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![login])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
