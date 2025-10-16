
pub mod brightwheel;

use std::{collections::VecDeque, path::{self, Path, PathBuf}, str::FromStr, sync::Arc};

use jiff::{Timestamp, Zoned};
use reqwest_cookie_store::CookieStoreMutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Builder, Emitter, Manager, State};

use exiftool::ExifTool;

use crate::brightwheel::{BrightwheelClient, Student};

type BackendReceiver = std::sync::mpsc::Receiver<BackendMessage>;
type BackendSender = std::sync::mpsc::Sender<BackendMessage>;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct FrontendState {
    message: Option<String>,
    output_dir: String,
    backend_state: BackendState,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
enum BackendState {
    NeedsLogin(NeedsLoginState),
    NeedsMfa(NeedsMfaState),
    LoggedIn(LoggedInState),
    Syncing(SyncingState),
    SyncCanceling(SyncCancelingState),
    UnexpectedError(String),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct NeedsLoginState {
    message: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct NeedsMfaState {
    email: String,
    password: String,
    message: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct LoggedInState {

}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SyncingState {

}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SyncCancelingState {

}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct UnexpectedErrorState {
    message: String,
}

#[derive(Serialize, Deserialize, Clone)]
enum BackendMessage {
    Test,
    DOMContentLoaded,
    LogIn {
        email: String,
        password: String,
    },
    LogInMfa {
        mfa_code: String,
    },
    SetOutputDir(String),
    Sync,
    DownloadStarted(PathBuf),
    SyncComplete,
    CancelSync,
    SyncCanceled,
    LogToFrontend(String),
    Exit
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (backend_sender, backend_receiver) = std::sync::mpsc::channel();
    let backend_sender_2 = backend_sender.clone();

    let app = Builder::default()
        .setup(|app| {
            app.manage(backend_sender);
            Ok(())            
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![send_backend_message]).build(
            tauri::generate_context!()
        )
        .expect("error while building tauri application");
    let app_handle = app.app_handle().clone();

    let config_dir = app_handle.path().app_config_dir().unwrap();
    if !config_dir.exists() {
        std::fs::create_dir_all(config_dir).unwrap();
    }

    std::thread::spawn(move || {
        run_backend(backend_sender_2, backend_receiver, app_handle);
    });
    app.run(|_app, _event| { });
}

fn run_backend(sender: std::sync::mpsc::Sender<BackendMessage>, receiver: std::sync::mpsc::Receiver<BackendMessage>, app: AppHandle) {
    let (already_logged_in, cookie_store) = init_cookie_store(&app);
    let mut bw_client = BrightwheelClient::new(cookie_store);
    let mut state = if already_logged_in {
        BackendState::LoggedIn(LoggedInState {  })
    }
    else {
        BackendState::NeedsLogin(NeedsLoginState {
            message: None
        })
    };

    // Launch sync engine thread
    let (sync_sender, sync_receiver) = std::sync::mpsc::channel();
    {
        let app_2 = app.clone();
        let output_root = output_dir(&app_2);
        let bw_client = bw_client.clone();
        std::thread::spawn(move || {
            run_sync_engine(app_2, output_root, bw_client, sync_receiver, sender);
        });
    }

    loop {
        let frontend_message = match receiver.recv().unwrap() {
            BackendMessage::Test => {
                respond_to_test_message(&app);
                Some("Test message".to_string())
            },
            BackendMessage::DOMContentLoaded => {    
                log_to_frontend(&app, format!("received notification of DOMContentLoaded on backend"));
                None
            },
            BackendMessage::LogIn { email, password } => {
                match state {
                    BackendState::NeedsLogin(needs_login_state) => {
                        state = needs_login_state.log_in(&app, &mut bw_client, email, password);
                    },
                    _ => {
                        log_to_frontend(&app, format!("LogIn received from wrong state: {:?}", state));
                    }
                };
                None
            },
            BackendMessage::LogInMfa { mfa_code } => {
                match state {
                    BackendState::NeedsMfa(needs_mfa_state) => {
                        state = needs_mfa_state.log_in_mfa(&app, &mut bw_client, mfa_code);
                        None
                    },
                    _ => {
                        log_to_frontend(&app, format!("LogInMfa received from wrong state: {:?}", state));
                        None
                    }
                }
            },
            BackendMessage::SetOutputDir(output_dir) => {
                set_output_dir(&app, PathBuf::from_str(&output_dir).unwrap());
                Some("Output directory set.".to_string())
            },
            BackendMessage::Sync => {
                state = sync(state, &sync_sender);
                None
            },
            BackendMessage::DownloadStarted(path) => {
                Some(path.to_str().unwrap().into())
            },
            BackendMessage::SyncComplete => {
                state = sync_complete(state);
                Some("Sync complete".to_string())
            },
            BackendMessage::CancelSync => {
                state = cancel_sync(state, &sync_sender);
                Some("Cancelling...".to_string())
            },
            BackendMessage::SyncCanceled => {
                state = sync_canceled(state);
                Some("Sync canceled.".to_string())
            },
            BackendMessage::LogToFrontend(msg) => {
                log_to_frontend(&app, msg);
                None
            }
            BackendMessage::Exit => {
                break;
            }
        };
        update_state(&app, &state, frontend_message);
    }
}


/*** ALL-PURPOSE BACKEND MESSAGING COMMAND ***/

#[tauri::command]
fn send_backend_message(sender: tauri::State<'_, BackendSender>, message: BackendMessage) -> Result<(), String> {
  sender.send(message).unwrap();
  Ok(())
}


/*** TEST REQUEST-RESPONSE ***/

#[derive(Serialize, Deserialize, Clone)]
struct TestEvent {
    message: String
}

fn respond_to_test_message(app: &AppHandle) {
    app.emit("test-event", TestEvent {
        message: "Hello to frontend".into()
    }).unwrap();
}

/*** FRONTEND DEBUG LOGGING ***/

fn log_to_frontend(app: &AppHandle, log_msg: String) {
    app.emit("log-event", log_msg).unwrap();
}

/*** FRONTEND STATE UPDATE ***/

fn update_state(app: &AppHandle, backend_state: &BackendState, frontend_message: Option<String>) {
    let frontend_state = FrontendState {
        message: frontend_message,
        output_dir: output_dir(app).to_str().unwrap().into(),
        backend_state: backend_state.clone(),
    };

    app.emit("update-state", frontend_state).unwrap();
}


/*** LOGIN ***/

// #[derive(Serialize, Deserialize, Clone, Debug)]
// enum LogInError {
//     WrongState
// }

// fn log_in(app: &AppHandle, bw_client: &mut BrightwheelClient, state: BackendState, email: String, password: String) -> Result<BackendState, LogInError> {
//     match state {
//     }
// }

impl NeedsLoginState {
    fn log_in(self, app: &AppHandle, bw_client: &mut BrightwheelClient, email: String, password: String) -> BackendState {
        let response = bw_client.post_sessions_start(email.clone(), password.clone());
        let response_json = response.json::<serde_json::Value>().unwrap();

        match response_json {
            serde_json::Value::Object(response_obj) => {
                if let Some(mfa_required_val) = response_obj.get("2fa_required") {
                    if let Some(mfa_required) = mfa_required_val.as_bool() {
                        if mfa_required {
                            BackendState::NeedsMfa(NeedsMfaState {
                                email, password, message: None
                            })
                        }
                        else {
                            complete_login(app, bw_client, email, password, None)
                        }
                    }
                    else {
                        BackendState::UnexpectedError("2fa_required is not a bool???".into())
                    }
                }
                else {
                    // TODO: this might actually be a login failure
                    BackendState::LoggedIn(LoggedInState {})
                }
            }
            _ => {
                BackendState::UnexpectedError("received non-object response from brightwheel login endpoint".into())
            }
        }
    }
}


// fn log_in_mfa(app: &AppHandle, bw_client: &mut BrightwheelClient, state: BackendState, mfa_code: String) -> BackendState {
//     match state {
//         BackendState::NeedsMfa(needs_mfa_state) => {
//             needs_mfa_state.log_in_mfa(app, bw_client, mfa_code)
//         },
//         _ => BackendState::UnexpectedError(format!("Backend in unexpected state for mfa login: {:?}", state)),
//     }
// }

impl NeedsMfaState {
    fn log_in_mfa(self, app: &AppHandle, bw_client: &mut BrightwheelClient, mfa_code: String) -> BackendState {
        complete_login(app, bw_client, self.email, self.password, Some(mfa_code))
    }
}


fn complete_login(app: &AppHandle, bw_client: &BrightwheelClient, email: String, password: String, mfa_code_opt: Option<String>) -> BackendState {
    let response = bw_client.post_sessions(email.clone(), password.clone(), mfa_code_opt.clone());
    let response_json = response.json::<serde_json::Value>().unwrap();
    println!("/sessions response_json: {}\n", serde_json::to_string(&response_json).unwrap());
    match response_json {
        serde_json::Value::Object(response_obj) => {
            // TODO: could be invalid response??
            write_cookies(app, &bw_client.cookie_store_arc_mutex);

            BackendState::LoggedIn(LoggedInState { })
        },
        _ => {
            BackendState::UnexpectedError("received non-object response from brightwheel login endpoint".into())
        }
    }
}

/*** SYNC ***/

#[derive(Serialize, Deserialize, Clone)]
enum SyncMessage {
    Sync,
    Cancel,
}

type SyncSender = std::sync::mpsc::Sender<SyncMessage>;
type SyncReceiver = std::sync::mpsc::Receiver<SyncMessage>;

struct SyncItem {
    student: Student,
    timestamp: Zoned,
    url: reqwest::Url,
    object_id: String,
    extension: String,
}

fn run_sync_engine(app: AppHandle, output_root: PathBuf, bw_client: BrightwheelClient, sync_receiver: SyncReceiver, backend_sender: BackendSender) {
    let exiftool_path: PathBuf = app.path().resource_dir().unwrap().join("exiftool").join("exiftool");
    println!("exiftool_path: {}", exiftool_path.to_str().unwrap());
    let mut exif_tool = ExifTool::with_executable(&exiftool_path).unwrap();

    let mut sync_engine = SyncEngine {
        output_root,
        bw_client,
        sync_receiver,
        backend_sender,
        exif_tool,
        download_items: VecDeque::new()
    };
    sync_engine.run();
}

struct SyncEngine {
    output_root: PathBuf,
    bw_client: BrightwheelClient,
    sync_receiver: SyncReceiver,
    backend_sender: BackendSender,
    exif_tool: ExifTool,
    download_items: VecDeque<SyncItem>,
}

impl SyncEngine {
    const PAGE_SIZE: usize = 1000;

    fn run(&mut self) {
        loop {
            if let Some(item) = self.download_items.pop_front() {
                self.download_item(item);
                if self.download_items.is_empty() {
                    self.backend_sender.send(BackendMessage::SyncComplete).unwrap();
                }
                
                match self.sync_receiver.try_recv() {
                    Ok(msg) => {
                        match msg {
                            SyncMessage::Sync => {
                                println!("Should not receive sync message while downloading");
                            },
                            SyncMessage::Cancel => {
                                self.download_items.clear();
                                self.backend_sender.send(BackendMessage::SyncCanceled);
                            },
                        }
                    },
                    Err(e) => {
                        match e {
                            std::sync::mpsc::TryRecvError::Empty => { },
                            std::sync::mpsc::TryRecvError::Disconnected => {
                                panic!("Should never get disconnected from sync engine channel");
                            },
                        }
                    },
                }
            }
            else {
                match self.sync_receiver.recv().unwrap() {
                    SyncMessage::Sync => {
                        self.sync();
                    },
                    SyncMessage::Cancel => {
                        println!("Nothing to cancel");
                    },
                }
            }
        }
    }

    fn sync(&mut self) {    
        // Get user_id
        let user_id = self.bw_client.get_user_id();
        println!("got user_id: {}", user_id);

        // Get list of students;
        let user_id_2 = user_id.clone();
        let students = self.bw_client.get_students(user_id_2);

        // Sync each student
        for student in students {
            self.sync_student(student);
        }
    }

    fn sync_student(&mut self, student: Student) {
        println!("sync_student: {} {}", student.first_name, student.last_name);
        self.enqueue_sync_items(&student);

        println!("...done");
    }

    fn enqueue_sync_items(&mut self, student: &Student) {
        println!("get_sync_items: {} {}", student.first_name, student.last_name);

        // let mut sync_items = Vec::new();
        let mut page: usize = 0;
        loop {
            let count = self.enqueue_sync_items_on_page(student, page);
            if count == 0 {
                break;
            }
            page += 1;
        }
    }

    fn enqueue_sync_items_on_page(&mut self, student: &Student, page: usize) -> usize {
        let response_json = self.bw_client.get_students_activities(
            student.object_id.clone(), Self::PAGE_SIZE, page
        ).json::<Value>().unwrap();
        let response_obj = response_json.as_object().unwrap();
        println!("response keys: {:?}", Vec::from_iter(response_obj.keys().into_iter()));

        let page = response_obj.get("page").unwrap().as_u64().unwrap() as usize;
        let page_size = response_obj.get("page_size").unwrap().as_u64().unwrap() as usize;
        println!("page, page_size: {}, {}", page, page_size);

        let activities = response_obj.get("activities").unwrap().as_array().unwrap();
        println!("# activities: {}", activities.len());

        let mut count: usize = 0;
        for activity in activities {
            if let Some(item) = self.get_sync_item_for_activity(student, activity.as_object().unwrap()) {
                self.download_items.push_back(item);
                count += 1;
            }
        }

        count
    }

    fn get_sync_item_for_activity(&mut self, student: &Student, activity: &Map<String, Value>) -> Option<SyncItem> {
        let timestamp = get_created_at(activity);
        let object_id = get_object_id(activity);

        let url_ext_opt: Option<(_, String)> = if activity.get("media").unwrap().is_object() {
            let photo_info = activity.get("media").unwrap().as_object().unwrap();
            let url = reqwest::Url::parse(photo_info.get("image_url").unwrap().as_str().unwrap()).unwrap();
            Some((url, "jpg".into()))
        }
        else if activity.get("video_info").unwrap().is_object() {
            let video_info = activity.get("video_info").unwrap().as_object().unwrap();
            let url = reqwest::Url::parse(video_info.get("downloadable_url").unwrap().as_str().unwrap()).unwrap();
            Some((url, "mp4".into()))
        }
        else {
            None
        };

        url_ext_opt.map(|(url, extension)| {
            SyncItem {
                student: student.clone(),
                timestamp,
                url,
                object_id,
                extension,
            }
        })
    }

    fn download_item(&mut self, item: SyncItem) {
        println!("todo: download {} {} {} {}.{}", item.student.first_name, item.student.last_name, item.timestamp, item.object_id, item.extension);
        
        let student_path = create_student_path(&self.output_root, &item.student);
        let month_path = create_month_path(&student_path, &item.timestamp);
        let filename = format_filename(&item.timestamp, &item.object_id,  &item.extension);
        let dst_path = month_path.join(filename);

        println!("{:?}", dst_path);
        if dst_path.exists() {
            println!("...already exists; skipping");
        }
        else {
            println!("...downloading...");
            self.backend_sender.send(BackendMessage::DownloadStarted(dst_path.clone())).unwrap();
            self.bw_client.download_file(&item.url, &dst_path);
            println!("...done.");
        }
    }

    fn log_to_frontend(&self, msg: String) {
        self.backend_sender.send(BackendMessage::LogToFrontend(msg)).unwrap();
    }
}

fn sync(state: BackendState, sync_sender: &SyncSender) -> BackendState {
    match state {
        BackendState::LoggedIn(logged_in_state) => {
            logged_in_state.sync(sync_sender)
        },
        _ => {
            BackendState::UnexpectedError(format!("Unexpected state for sync: {:?}", state))
        }
    }
}

fn cancel_sync(state: BackendState, sync_sender: &SyncSender) -> BackendState {
    match state {
        BackendState::Syncing(syncing_state) => {
            syncing_state.cancel_sync(sync_sender)
        },
        _ => {
            BackendState::UnexpectedError(format!("Unexpected state for cancel sync: {:?}", state))
        }
    }
}

fn sync_canceled(state: BackendState) -> BackendState {
    match state {
        BackendState::SyncCanceling(sync_canceling_state) => {
            sync_canceling_state.sync_canceled()
        },
        _ => {
            BackendState::UnexpectedError(format!("Unexpected state for sync canceled: {:?}", state))
        }
    }
}

fn sync_complete(state: BackendState) -> BackendState {
    match state {
        BackendState::Syncing(syncing_state) => {
            syncing_state.sync_complete()
        },
        _ => {
            BackendState::UnexpectedError(format!("Unexpected state for cancel sync: {:?}", state))
        }
    }
}

impl LoggedInState {
    fn sync(self, sync_sender: &SyncSender) -> BackendState {
        sync_sender.send(SyncMessage::Sync).unwrap();
        BackendState::Syncing(SyncingState { })
    }
}

impl SyncingState {
    fn cancel_sync(self, sync_sender: &SyncSender) -> BackendState {
        sync_sender.send(SyncMessage::Cancel).unwrap();
        BackendState::SyncCanceling(SyncCancelingState { })
    }

    fn sync_complete(self) -> BackendState {
        BackendState::LoggedIn(LoggedInState { })
    }
}

impl SyncCancelingState {
    fn sync_canceled(self) -> BackendState {
        BackendState::LoggedIn(LoggedInState { })
    }
}

fn sync_all(app: &AppHandle, bw_client: &mut BrightwheelClient) {
    let exiftool_path: PathBuf = "../exiftool/exiftool".into();
    println!("exiftool_path: {}", exiftool_path.to_str().unwrap());
    let mut exif_tool = ExifTool::with_executable(&exiftool_path).unwrap();

    // Get user_id
    let user_id = bw_client.get_user_id();
    println!("got user_id: {}", user_id);

    // Get list of students;
    let user_id_2 = user_id.clone();
    let students = bw_client.get_students(user_id_2);

    // Sync each student
    for student in students {
        sync_student(app, &bw_client, &mut exif_tool, student);
    }
}

fn sync_student(app: &AppHandle, bw_client: &BrightwheelClient, exif_tool: &mut ExifTool, student: Student) {
    println!("sync_student: {} {}", student.first_name, student.last_name);

    let student_path = output_dir(app).join(format!("{} {}", student.first_name, student.last_name));
    if !student_path.exists() {
        std::fs::create_dir(&student_path).unwrap();
    }

    let page_size: usize = 1000;
    let mut page: usize = 0;

    while download_activities(bw_client, &student, page_size, page, &student_path) {
        page += 1;
    }

    println!("Updating exif data for student...");
    let output = exif_tool.execute_lines(&[
        "-r", "-overwrite_original", "-alldates<filename",
        "-gpsposition=37.78401801046647, -122.50330791369049",
        student_path.to_str().unwrap()
    ]).unwrap();
    for line in output {
        println!("{}", line);
    }
    println!("...done");
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
    println!("timestamp: {:?}", timestamp);
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


fn format_filename(timestamp: &Zoned, object_id: &str, extension: &str) -> String {
    format!("{}-{}.{}", timestamp.strftime("%F-%H%M%S").to_string(), object_id, extension)
}

fn get_object_id(obj: &Map<String, Value>) -> String {
    obj.get("object_id").unwrap().as_str().unwrap().into()
}

fn get_created_at(obj: &Map<String, Value>) -> Zoned {
    let timestamp: Timestamp = obj.get("created_at").unwrap().as_str().unwrap().parse().unwrap();
    timestamp.in_tz("America/Los_Angeles").unwrap()
}

fn get_month_path(root_dir: &PathBuf, ts: &Zoned) -> PathBuf {
    let month_str = ts.strftime("%Y-%m").to_string();
    root_dir.join(month_str)
}

fn create_student_path(root_dir: &PathBuf, student: &Student) -> PathBuf {
    let student_path = root_dir.join(format!("{} {}", student.first_name, student.last_name));
    if !student_path.exists() {
        std::fs::create_dir_all(&student_path).unwrap();
    }
    student_path
}

fn create_month_path(root_dir: &PathBuf, ts: &Zoned) -> PathBuf {
    let month_path = get_month_path(root_dir, ts);
    if !month_path.exists() {
        std::fs::create_dir_all(&month_path).unwrap();
    }
    month_path
}


/*** UTILITY FUNCTIONS ***/

fn config_dir(app: &AppHandle) -> PathBuf {
    app.path().app_config_dir().unwrap()
}

fn config_path(app: &AppHandle) -> PathBuf {
    config_dir(app).join("config.json")
}

fn config(app: &AppHandle) -> serde_json::Value {
    let config_path = config_path(app);
    if config_path.exists() {
        let file = std::fs::File::open(&config_path).unwrap();
        serde_json::from_reader(file).unwrap()
    }
    else {
        json!({})
    }
}

fn write_config(app: &AppHandle, config: serde_json::Value) {
    let config_path = config_path(app);
    let file = std::fs::File::options().create(true).truncate(true).open(&config_path).unwrap();
    serde_json::to_writer(file, &config).unwrap();
}

fn set_output_dir(app: &AppHandle, output_dir: PathBuf) {
    let config = json!({"output_dir" : output_dir.to_str().unwrap() });
    write_config(app, config);
}

fn cookies_path(app: &AppHandle) -> PathBuf {
    app.path().app_config_dir().unwrap().join("cookies.json")
}

fn output_dir(app: &AppHandle) -> PathBuf {
    let config = config(app);
    let config_map = config.as_object().unwrap();

    if config_map.contains_key("output_dir") {
        config_map.get("output_dir").unwrap().as_str().unwrap().into()
    }
    else {
        default_output_dir(app)
    }
}

fn default_output_dir(app: &AppHandle) -> PathBuf {
    app.path().picture_dir().unwrap().join("shinydisc")
}

fn write_cookies(app: &AppHandle, cookie_store_arc_mutex: &Arc<CookieStoreMutex>) {
    let mut writer = std::fs::File::create(cookies_path(app))
      .map(std::io::BufWriter::new)
      .unwrap();
    cookie_store_arc_mutex.lock().unwrap().save_json(&mut writer);
}

fn to_json_debug<S: Serialize>(x: &S) -> String {
    serde_json::to_string_pretty(x).unwrap()
}

fn init_cookie_store(app: &AppHandle) -> (bool, reqwest_cookie_store::CookieStore) {
    if let Ok(file) = std::fs::File::open(cookies_path(app))
        .map(std::io::BufReader::new) {
        println!("Opened cookies.json");
        
        (true, reqwest_cookie_store::CookieStore::load_json(file).unwrap())
    }
    else
    {
        println!("No cookies.json; using default cookie store");
        (false, reqwest_cookie_store::CookieStore::default())
    }
}
