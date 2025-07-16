
pub mod brightwheel;

use std::sync::{Mutex};

use serde::Serialize;
use tauri::{ipc::{InvokeResponseBody, IpcResponse}, Builder, Manager, State};

use crate::brightwheel::BrightwheelClient;

struct OuterAppState {
  state_opt: Option<AppState>,
}

enum AppState {
    Start(StartState),
    NeedsMfa(NeedsMfaState),
    LoggedIn(LoggedInState),
    Error(String),
}

struct StartState {
    bw_client: BrightwheelClient
}

impl StartState {
    fn login(self, email: &str, password: &str) -> AppState {
        let bw_client = self.bw_client;

        let response = bw_client.authenticate_email_password(email, password);
        let response_json = response.json::<serde_json::Value>().unwrap();

        match response_json {
            serde_json::Value::Object(response_obj) => {
                if let Some(mfa_required_val) = response_obj.get("2fa_required") {
                    if let Some(mfa_required) = mfa_required_val.as_bool() {
                        if mfa_required {
                            AppState::NeedsMfa(NeedsMfaState { bw_client })
                        }
                        else {
                            AppState::LoggedIn(LoggedInState { bw_client })
                        }
                    }
                    else {
                        AppState::Error("2fa_required is not a bool???".into())
                    }
                }
                else {
                    // TODO: this might actually be a login failure
                    AppState::LoggedIn(LoggedInState { bw_client })
                }
            }
            _ => {
                AppState::Error("received non-object response from brightwheel login endpoint".into())
            }
        }
        
    }
}

struct NeedsMfaState {
    bw_client: BrightwheelClient
}
struct LoggedInState {
    bw_client: BrightwheelClient
}

#[derive(Serialize)]
struct LoginResult {
    message: Option<String>,
    tab_name: String,
}

/*impl IpcResponse for LoginResult {
    fn body(self) -> tauri::Result<tauri::ipc::InvokeResponseBody> {
        Ok(InvokeResponseBody::Json(serde_json::to_string(&self).unwrap()))
    }
}*/

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn login(state_mutex: State<'_, Mutex<OuterAppState>>, email: &str, password: &str) -> LoginResult {
    let mut outer_state = state_mutex.lock().unwrap();

    outer_state.state_opt = Some(if let Some(state) = outer_state.state_opt.take() {
        match state {
            AppState::Start(start_state) => {
                start_state.login(email, password)
            },
            _ => AppState::Error("wrong state for login".into())
        }
    }
    else {
        AppState::Error("outer state is empty for some reason?".into())
    });

    if let Some(state) = &outer_state.state_opt {
        match state {
            AppState::Error(msg) => LoginResult {
                message: Some(msg.clone()),
                tab_name: "login".into(),
            },
            AppState::NeedsMfa(_) => LoginResult {
                message: None,
                tab_name: "mfa".into(),
            },
            AppState::LoggedIn(_) => LoginResult {
                message: None,
                tab_name: "loggedin".into(),
            },
            _ => LoginResult {
                message: None,
                tab_name: "login".into(),
            }
        }
    }
    else {
        LoginResult {
            message: Some("outer state is empty for some reason?".into()),
            tab_name: "login".into(),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    Builder::default()
        .setup(|app| {
            app.manage(Mutex::new(OuterAppState {
                state_opt: Some(
                    AppState::Start(StartState {
                        bw_client: brightwheel::BrightwheelClient::new()
                    })
                )
            }));
            Ok(())            
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![login])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
