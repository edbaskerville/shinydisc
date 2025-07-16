
pub mod brightwheel;

use std::sync::{Mutex};

use serde::Serialize;
use tauri::{Builder, Manager, State};

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

fn complete_login(bw_client: BrightwheelClient, email: &str, password: &str, mfa_code_opt: Option<&str>) -> AppState {
    let response = bw_client.post_sessions(email, password, mfa_code_opt);
    let response_json = response.json::<serde_json::Value>().unwrap();
    println!("/sessions response_json: {}\n", serde_json::to_string(&response_json).unwrap());
    match response_json {
        serde_json::Value::Object(response_obj) => {
            AppState::LoggedIn(LoggedInState { bw_client })
        },
        _ => {
            AppState::Error("received non-object response from brightwheel login endpoint".into())
        }
    }
}

impl StartState {
    fn login(self, email: &str, password: &str) -> AppState {
        let bw_client = self.bw_client;

        let response = bw_client.post_sessions_start(email, password);
        let response_json = response.json::<serde_json::Value>().unwrap();
        println!("/sessions/start response_json: {}\n", serde_json::to_string(&response_json).unwrap());

        match response_json {
            serde_json::Value::Object(response_obj) => {
                if let Some(mfa_required_val) = response_obj.get("2fa_required") {
                    if let Some(mfa_required) = mfa_required_val.as_bool() {
                        if mfa_required {
                            AppState::NeedsMfa(NeedsMfaState { bw_client })
                        }
                        else {
                            complete_login(bw_client, email, password, None)
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

impl NeedsMfaState {
    fn complete_login(self, email: &str, password: &str, mfa_code: &str) -> AppState {
        let bw_client = self.bw_client;
        complete_login(bw_client, email, password, Some(mfa_code))
    }
}
struct LoggedInState {
    bw_client: BrightwheelClient
}

#[derive(Serialize)]
struct LoginResult {
    message: Option<String>,
    tab_name: String,
}

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

#[derive(Serialize)]
struct LoginMfaResult {
    message: Option<String>,
    tab_name: String,
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn login_mfa(state_mutex: State<'_, Mutex<OuterAppState>>, email: &str, password: &str, mfa_code: &str) -> LoginMfaResult {
    println!("login_mfa({}, ***, {})", email, mfa_code);
    let mut outer_state = state_mutex.lock().unwrap();

    outer_state.state_opt = Some(if let Some(state) = outer_state.state_opt.take() {
        match state {
            AppState::NeedsMfa(needs_mfa_state) => {
                needs_mfa_state.complete_login(email, password, mfa_code)
            },
            _ => AppState::Error("wrong state for login_mfa".into())
        }
    }
    else {
        AppState::Error("outer state is empty for some reason?".into())
    });

    if let Some(state) = &outer_state.state_opt {
        match state {
            AppState::Error(msg) => LoginMfaResult {
                message: Some(msg.clone()),
                tab_name: "mfa".into(),
            },
            AppState::LoggedIn(_) => LoginMfaResult {
                message: None,
                tab_name: "loggedin".into(),
            },
            _ => LoginMfaResult {
                message: None,
                tab_name: "mfa".into(),
            }
        }
    }
    else {
        LoginMfaResult {
            message: Some("outer state is empty for some reason?".into()),
            tab_name: "mfa".into(),
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
        .invoke_handler(tauri::generate_handler![login, login_mfa])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
