
pub mod brightwheel;

use std::{fs, ops::Deref, path::{Path, PathBuf}, sync::{Arc, Mutex}};

use jiff::{civil::Time, Timestamp};
use reqwest_cookie_store::CookieStoreMutex;
use serde::Serialize;
use serde_json::{Map, Value};
use tauri::{Builder, Manager, State};

use crate::brightwheel::{BrightwheelClient, Student};

fn to_json_debug<S: Serialize>(x: &S) -> String {
    serde_json::to_string_pretty(x).unwrap()
}

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

fn complete_login(mut bw_client: BrightwheelClient, email: &str, password: &str, mfa_code_opt: Option<&str>) -> AppState {
    let response = bw_client.post_sessions(email, password, mfa_code_opt);
    let response_json = response.json::<serde_json::Value>().unwrap();
    println!("/sessions response_json: {}\n", serde_json::to_string(&response_json).unwrap());
    match response_json {
        serde_json::Value::Object(response_obj) => {
            write_cookies(&bw_client.cookie_store_arc_mutex);

            AppState::LoggedIn(LoggedInState { bw_client })
        },
        _ => {
            AppState::Error("received non-object response from brightwheel login endpoint".into())
        }
    }
}

fn write_cookies(cookie_store_arc_mutex: &Arc<CookieStoreMutex>) {
    let mut writer = std::fs::File::create("cookies.json")
      .map(std::io::BufWriter::new)
      .unwrap();
    cookie_store_arc_mutex.lock().unwrap().save_json(&mut writer);
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
struct InitViewResult {
    tab_name: String,
}

#[tauri::command]
fn init_view(state_mutex: State<'_, Mutex<OuterAppState>>) -> InitViewResult {
    let outer_state = state_mutex.lock().unwrap();
    let tab_name = if let Some(state) = &outer_state.state_opt {
        match state {
            AppState::Start(_) => "login",
            AppState::NeedsMfa(_) => "mfa",
            AppState::LoggedIn(_) => "loggedin",
            AppState::Error(_) => "login",
        }
    }
    else {
        "login"
    };
    InitViewResult { tab_name: tab_name.into() }
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

#[derive(Serialize)]
struct SyncResult {
    user_id: Option<String>,
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn sync(state_mutex: State<'_, Mutex<OuterAppState>>) -> SyncResult {
    if let AppState::LoggedIn(logged_in_state) = state_mutex.lock().unwrap().state_opt.as_ref().unwrap() {
        let bw_client = &logged_in_state.bw_client;
        
        let user_id = bw_client.get_user_id();
        println!("got user_id: {}", user_id);

        let students = bw_client.get_students(&user_id);
        for student in &students {
            sync_student(bw_client, student)
        }

        SyncResult {
            user_id: None,
        }
    }
    else {
        SyncResult {
            user_id: None,
        }
    }
}

fn sync_student(bw_client: &BrightwheelClient, student: &Student) {
    println!("sync_student: {} {}", student.first_name, student.last_name);

    let student_path = PathBuf::from(format!("{} {}", student.first_name, student.last_name));
    if !student_path.exists() {
        std::fs::create_dir(&student_path).unwrap();
    }

    let page_size: usize = 1000;
    let mut page: usize = 0;

    while download_activities(bw_client, student, page_size, page, &student_path) {
        page += 1;
    }
}

fn download_activities(bw_client: &BrightwheelClient, student: &Student, page_size: usize, page: usize, path: &PathBuf) -> bool {
    println!("download_activities: {} {}, page {}", student.first_name, student.last_name, page);

    let response_json = bw_client.get_students_activities(
        &student.object_id, page_size, page
    ).json::<Value>().unwrap();
    let response_obj = response_json.as_object().unwrap();
    println!("response keys: {:?}", Vec::from_iter(response_obj.keys().into_iter()));

    let page = response_obj.get("page").unwrap().as_u64().unwrap() as usize;
    let page_size = response_obj.get("page_size").unwrap().as_u64().unwrap() as usize;
    println!("page, page_size: {}, {}", page, page_size);

    let activities = response_obj.get("activities").unwrap().as_array().unwrap();
    println!("# activities: {}", activities.len());
    for (i, activity_val) in activities.iter().enumerate() {
        let activity = activity_val.as_object().unwrap();
        println!("page {}, item {}", page, i);
        println!("activity keys: {:?}", Vec::from_iter(activity.keys().into_iter()));
        if activity.get("media").unwrap().is_object() {
            println!("found media");
            download_photo(bw_client, student, path, activity);
        }
        else if activity.get("video_info").unwrap().is_object() {
            println!("found video_info");
            download_video(bw_client, student, path, activity);
        }
        // println!("activity keys: {:?}", Vec::from_iter(activity.keys().into_iter()));
        // println!("activity: {:?}", activity);

        // if(i > 10) {
        //     break;
        // }
    }

    activities.len() == page_size
}

fn download_photo(bw_client: &BrightwheelClient, student: &Student, path: &PathBuf, activity: &Map<String, Value>) {
    let timestamp = get_created_at(activity);
    let object_id = get_object_id(activity);
    let month_path = create_month_path(path, &timestamp);
    let photo_info = activity.get("media").unwrap().as_object().unwrap();
    // println!("{}\n", to_json_debug(photo_info));

    let src_url = reqwest::Url::parse(photo_info.get("image_url").unwrap().as_str().unwrap()).unwrap();
    let filename = format_filename(&timestamp, &object_id, "jpg");
    let dst_path = month_path.join(filename);

    println!("{:?}", dst_path);
    if dst_path.exists() {
        println!("...already exists; skipping");
    }
    else {
        println!("...downloading...");
        bw_client.download_file(&src_url, &dst_path);
        println!("...done.");
    }
}

fn download_video(bw_client: &BrightwheelClient, student: &Student, path: &PathBuf, activity: &Map<String, Value>) {
    let timestamp = get_created_at(activity);
    let object_id = get_object_id(activity);
    let month_path = create_month_path(path, &timestamp);
    let video_info = activity.get("video_info").unwrap().as_object().unwrap();
    println!("{}\n", to_json_debug(video_info));

    let src_url = reqwest::Url::parse(video_info.get("downloadable_url").unwrap().as_str().unwrap()).unwrap();
    let filename = format_filename(&timestamp, &object_id, "mp4");
    let dst_path = month_path.join(filename);

    println!("{:?}", dst_path);
    if dst_path.exists() {
        println!("...already exists; skipping");
    }
    else {
        println!("...downloading...");
        bw_client.download_file(&src_url, &dst_path);
        println!("...done.");
    }
}


fn format_filename(timestamp: &Timestamp, object_id: &str, extension: &str) -> String {
    format!("{}-{}.{}", timestamp.strftime("%F-%H%M%S").to_string(), object_id, extension)
}

fn get_object_id(obj: &Map<String, Value>) -> String {
    obj.get("object_id").unwrap().as_str().unwrap().into()
}

fn get_created_at(obj: &Map<String, Value>) -> Timestamp {
    obj.get("created_at").unwrap().as_str().unwrap().parse().unwrap()
}

fn get_month_path(path: &PathBuf, ts: &Timestamp) -> PathBuf {
    let month_str = ts.strftime("%Y-%m").to_string();
    path.join(month_str)
}

fn create_month_path(path: &PathBuf, ts: &Timestamp) -> PathBuf {
    let month_path = get_month_path(path, ts);
    if !month_path.exists() {
        std::fs::create_dir(&month_path).unwrap();
    }
    month_path
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let start_state = {
        if let Ok(file) = std::fs::File::open("cookies.json")
            .map(std::io::BufReader::new) {
            println!("Opened cookies.json");

            AppState::LoggedIn(LoggedInState {
                bw_client: brightwheel::BrightwheelClient::new(
                    reqwest_cookie_store::CookieStore::load_json(file).unwrap()
                )
            })
        }
        else
        {
            println!("No cookies.json; using default cookie store");
            AppState::Start(StartState {
                bw_client: brightwheel::BrightwheelClient::new(reqwest_cookie_store::CookieStore::default())
            })
        }
    };

    Builder::default()
        .setup(|app| {
            app.manage(Mutex::new(OuterAppState {
                state_opt: Some(start_state)
            }));
            Ok(())            
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![init_view, login, login_mfa, sync])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
