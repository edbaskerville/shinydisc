
pub mod brightwheel;

use std::{fs, ops::Deref, path::{Path, PathBuf}, sync::{Arc}};

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
    bw_client_opt: Option<BrightwheelClient>,
    state_opt: Option<AppState>,
}

impl OuterAppState {
    pub fn remove(&mut self) -> (BrightwheelClient, AppState) {
        (
            self.bw_client_opt.take().unwrap(),
            self.state_opt.take().unwrap(),
        )
    }

    pub fn insert(&mut self, bw_client: BrightwheelClient, state: AppState) {
        self.bw_client_opt = Some(bw_client);
        self.state_opt = Some(state);
    }
}

enum AppState {
    Start(StartState),
    NeedsMfa(NeedsMfaState),
    LoggedIn(LoggedInState),
    Error(String),
}

struct StartState {}

fn complete_login(bw_client: &BrightwheelClient, email: String, password: String, mfa_code_opt: Option<String>) -> AppState {
    let response = bw_client.post_sessions(email.clone(), password.clone(), mfa_code_opt.clone());
    let response_json = response.json::<serde_json::Value>().unwrap();
    println!("/sessions response_json: {}\n", serde_json::to_string(&response_json).unwrap());
    match response_json {
        serde_json::Value::Object(response_obj) => {
            write_cookies(&bw_client.cookie_store_arc_mutex);

            AppState::LoggedIn(LoggedInState { })
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
    fn login(self, bw_client: &BrightwheelClient, email: String, password: String) -> AppState {
        let response = bw_client.post_sessions_start(email.clone(), password.clone());
        let response_json = response.json::<serde_json::Value>().unwrap();
        println!("/sessions/start response_json: {}\n", serde_json::to_string(&response_json).unwrap());

        match response_json {
            serde_json::Value::Object(response_obj) => {
                if let Some(mfa_required_val) = response_obj.get("2fa_required") {
                    if let Some(mfa_required) = mfa_required_val.as_bool() {
                        if mfa_required {
                            AppState::NeedsMfa(NeedsMfaState { })
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
                    AppState::LoggedIn(LoggedInState { })
                }
            }
            _ => {
                AppState::Error("received non-object response from brightwheel login endpoint".into())
            }
        }
        
    }
}

struct NeedsMfaState { }

impl NeedsMfaState {
    fn complete_login(self, bw_client: &BrightwheelClient, email: String, password: String, mfa_code: String) -> AppState {
        complete_login(bw_client, email, password, Some(mfa_code))
    }
}
struct LoggedInState { }

#[derive(Serialize)]
struct InitViewResponse {
    tab_name: String,
}

#[tauri::command]
async fn init_view(state_mutex: State<'_, tokio::sync::Mutex<OuterAppState>>) -> Result<InitViewResponse, ()> {
    let outer_state = state_mutex.lock().await;
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
    Ok(InitViewResponse { tab_name: tab_name.into() })
}

#[derive(Serialize)]
struct LoginResponse {
    message: Option<String>,
    tab_name: String,
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
async fn login(state_mutex: State<'_, tokio::sync::Mutex<OuterAppState>>, email: String, password: String) -> Result<LoginResponse, ()> {
    let mut outer_state = state_mutex.lock().await;
    let (mut bw_client, mut state) = outer_state.remove();

    (bw_client, state) = match state {
        AppState::Start(start_state) => {
            let handle = tokio::task::spawn_blocking(move || {
                let state = start_state.login(&bw_client, email, password);
                (bw_client, state)
            });
            handle.await.unwrap()
        },
        _ => (bw_client, AppState::Error("wrong state for login".into()))
    };

    let response = match &state {
        AppState::Error(msg) => LoginResponse {
            message: Some(msg.clone()),
            tab_name: "login".into(),
        },
        AppState::NeedsMfa(_) => LoginResponse {
            message: None,
            tab_name: "mfa".into(),
        },
        AppState::LoggedIn(_) => LoginResponse {
            message: None,
            tab_name: "loggedin".into(),
        },
        _ => LoginResponse {
            message: None,
            tab_name: "login".into(),
        }
    };
    outer_state.insert(bw_client, state);
    Ok(response)
}

#[derive(Serialize)]
struct LoginMfaResponse {
    message: Option<String>,
    tab_name: String,
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
async fn login_mfa(state_mutex: State<'_, tokio::sync::Mutex<OuterAppState>>, email: String, password: String, mfa_code: String) -> Result<LoginMfaResponse, ()> {
    println!("login_mfa({}, ***, {})", email, mfa_code);
    let mut outer_state = state_mutex.lock().await;
    let (mut bw_client, mut state) = outer_state.remove();

    // TODO: This pattern of extracting state probably assumes that no commands get executed in here.
    // Probably need to restructure using a real message queue.
    // Probably should receive messages on a normal thread to process the queue and avoid async ownership nonsense.
    // As usual, weird code means you're doing something wrong.
    // Wait, nevermind, we hold the lock the whole time.
    // The locking implicitly creates a queue.
    // Still, ugly.

    (bw_client, state) = match state {
        AppState::NeedsMfa(needs_mfa_state) => {
            tokio::task::spawn_blocking(|| {
                let state = needs_mfa_state.complete_login(&bw_client, email, password, mfa_code);
                (bw_client, state)
            }).await.unwrap()
        },
        _ => (bw_client, AppState::Error("wrong state for login_mfa".into()))
    };

    let response = match &state {
        AppState::Error(msg) => LoginMfaResponse {
            message: Some(msg.clone()),
            tab_name: "mfa".into(),
        },
        AppState::LoggedIn(_) => LoginMfaResponse {
            message: None,
            tab_name: "loggedin".into(),
        },
        _ => LoginMfaResponse {
            message: None,
            tab_name: "mfa".into(),
        }
    };

    outer_state.insert(bw_client, state);
    Ok(response)
}

#[derive(Serialize)]
struct SyncResponse {
    user_id: Option<String>,
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
async fn sync(state_mutex: State<'_, tokio::sync::Mutex<OuterAppState>>) -> Result<SyncResponse, ()> {
    let response = {
        let mut outer_state = state_mutex.lock().await;
        let (mut bw_client, mut state) = outer_state.remove();
        
        let user_id;
        (bw_client, user_id) = tokio::task::spawn_blocking(move || {
            let user_id = bw_client.get_user_id();
            (bw_client, user_id)
        }).await.unwrap();
        println!("got user_id: {}", user_id);

        let students;
        let user_id_2 = user_id.clone();
        (bw_client, students) = tokio::task::spawn_blocking(|| {
            let students = bw_client.get_students(user_id_2);
            (bw_client, students)
        }).await.unwrap();
        for student in students {
            bw_client = tokio::task::spawn_blocking(move || {
                sync_student(&bw_client, student);
                bw_client
            }).await.unwrap()
        }

        outer_state.insert(bw_client, state);
        SyncResponse {
            user_id: Some(user_id),
        }
    };

    Ok(response)
}

fn sync_student(bw_client: &BrightwheelClient, student: Student) {
    println!("sync_student: {} {}", student.first_name, student.last_name);

    let student_path = PathBuf::from(format!("{} {}", student.first_name, student.last_name));
    if !student_path.exists() {
        std::fs::create_dir(&student_path).unwrap();
    }

    let page_size: usize = 1000;
    let mut page: usize = 0;

    while download_activities(bw_client, &student, page_size, page, &student_path) {
        page += 1;
    }
}

fn download_activities(bw_client: &BrightwheelClient, student: &Student, page_size: usize, page: usize, path: &PathBuf) -> bool {
    println!("download_activities: {} {}, page {}", student.first_name, student.last_name, page);

    let response_json = bw_client.get_students_activities(
        student.object_id.clone(), page_size, page
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
    

    let outer_state = {
        if let Ok(file) = std::fs::File::open("cookies.json")
            .map(std::io::BufReader::new) {
            println!("Opened cookies.json");

            OuterAppState {
                bw_client_opt: Some(brightwheel::BrightwheelClient::new(
                    reqwest_cookie_store::CookieStore::load_json(file).unwrap()
                )),
                state_opt: Some(AppState::LoggedIn(LoggedInState { }))
            }
        }
        else
        {
            println!("No cookies.json; using default cookie store");
            OuterAppState {
                bw_client_opt: Some(
                    brightwheel::BrightwheelClient::new(reqwest_cookie_store::CookieStore::default())
                ),
                state_opt: Some(AppState::Start(StartState { }))
            }
        }
    };

    Builder::default()
        .setup(|app| {
            app.manage(tokio::sync::Mutex::new(outer_state));
            Ok(())            
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![init_view, login, login_mfa, sync])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
