
pub mod brightwheel;

use std::{fs, path::PathBuf, str::FromStr, sync::Arc, time::{Duration, SystemTime}};

use jiff::{Timestamp, Zoned};
use reqwest_cookie_store::CookieStoreMutex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Builder, Emitter, Manager, Url, webview::{WebviewWindowBuilder, Cookie}, utils::config::WebviewUrl};

use exiftool::ExifTool;

use crate::brightwheel::{BrightwheelClient, Student};

type BackendReceiver = std::sync::mpsc::Receiver<BackendMessage>;
type BackendSender = std::sync::mpsc::Sender<BackendMessage>;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct FrontendState {
    message: Option<String>,
    output_dir: String,
    update_all_metadata: bool,
    gps_coords: String,
    backend_state: BackendState,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
enum BackendState {
    LoggedOut(LoggedOutState),
    LoggedIn(LoggedInState),
    Syncing(SyncingState),
    SyncCanceling(SyncCancelingState),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct LoggedOutState {
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

#[derive(Serialize, Deserialize, Clone)]
enum BackendMessage {
    Test,
    OnBrightwheelNavigation(Url),
    DOMContentLoaded,
    SetOutputDir(String),
    SetUpdateAllMetadata(bool),
    SetGPSCoords(String),
    Sync,
    QueryingItems {
        page: usize,
    },
    QueriedItems {
        page: usize,
        count: usize,
    },
    ProcessingItem {
        needs_download: bool,
        path: PathBuf,
        index: usize,
        count: usize,
    },
    SyncComplete,
    SyncError(String),
    CancelSync,
    SyncCanceled,
    LogToFrontend(String),
    Exit
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (backend_sender, backend_receiver) = std::sync::mpsc::channel();
    let backend_sender_app_manage = backend_sender.clone();
    let backend_sender_on_navigation = backend_sender.clone();

    let app = Builder::default()
        .setup(|app| {
            app.manage(backend_sender_app_manage);

            let _wvw = WebviewWindowBuilder::new(
                app, "main",
                WebviewUrl::App("index.html".into())
            ).on_navigation(move |url| {
                backend_sender_on_navigation.send(
                    BackendMessage::OnBrightwheelNavigation(url.clone())
                ).unwrap();

                true
            }).title("shinydisc").build();

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
        run_backend(backend_sender, backend_receiver, app_handle);
    });
    app.run(|_app, _event| { });
}

pub fn update_cookies(cookie_store_arc_mutex: &Arc<CookieStoreMutex>, cookies: Vec<tauri::webview::Cookie<'static>>) {
    let mut guard = cookie_store_arc_mutex.lock().unwrap();
    guard.clear();
    let request_url = Url::parse(brightwheel::BRIGHTWHEEL_URL_BASE).unwrap();
    for cookie in cookies {
        if cookie.name().eq_ignore_ascii_case("_brightwheel_v2") {
            let cookie_str = cookie.to_string();
            println!("cookie_str = {}", cookie_str);
            let cookie = cookie_store::Cookie::parse(cookie_str, &request_url).unwrap();
            guard.insert(
                cookie, &request_url
            ).unwrap();
        }
    }
}

fn run_backend(sender: BackendSender, receiver: BackendReceiver, app: AppHandle) {
    let cookie_store_arc_mutex = Arc::new(
        CookieStoreMutex::new(reqwest_cookie_store::CookieStore::new())
    );
    let mut bw_client = BrightwheelClient::new(cookie_store_arc_mutex.clone());
    let mut state = BackendState::LoggedOut(LoggedOutState { });

    // Launch sync engine thread
    let (io_sender, io_receiver) = std::sync::mpsc::channel();
    {
        let app_2 = app.clone();
        let output_root = get_output_dir(&app_2);
        std::thread::spawn(move || {
            run_sync_engine(app_2, output_root, bw_client, io_receiver, sender);
        });
    }

    loop {
        let frontend_message = match receiver.recv().unwrap() {
            BackendMessage::Test => {
                respond_to_test_message(&app);
                Some("Test message".to_string())
            },
            BackendMessage::OnBrightwheelNavigation(url) => {
                println!("navigation url: {:?}", url);
                let cookies = app.get_webview_window("main").unwrap().cookies().unwrap().to_owned();
                state = update_login_state_from_cookies(&app, state, &cookies);
                update_cookies(&cookie_store_arc_mutex, cookies);
                None
            },
            BackendMessage::DOMContentLoaded => {
                log_to_frontend(&app, format!("received notification of DOMContentLoaded on backend"));
                None
            },
            BackendMessage::SetOutputDir(output_dir) => {
                set_output_dir(&app, PathBuf::from_str(&output_dir).unwrap());
                Some("Output directory set.".to_string())
            },
            BackendMessage::SetUpdateAllMetadata(update_all_metdata) => {
                set_update_all_metadata(&app, update_all_metdata);
                None
            },
            BackendMessage::SetGPSCoords(gps_coords) => {
                set_gps_coords(&app, gps_coords);
                Some("GPS coordinates set.".to_string())
            },
            BackendMessage::Sync => {
                state = sync(state, &io_sender);
                None
            },
            BackendMessage::QueryingItems { page } => {
                Some(format!("Querying items from page {}", page + 1))
            },
            BackendMessage::QueriedItems { page, count } => {
                Some(format!("Query found {} items on page {}", count, page + 1))
            },
            BackendMessage::ProcessingItem {
                needs_download,
                path,
                index,
                count,
            } => {
                let base_msg = format!("{} ({}/{})", path.to_str().unwrap().to_string(), index + 1, count);
                if needs_download {
                    Some(base_msg)
                }
                else {
                    Some(format!("{} - already downloaded", base_msg))
                }
            },
            BackendMessage::SyncComplete => {
                state = sync_complete(state);
                Some("Sync complete".to_string())
            },
            BackendMessage::SyncError(message) => {
                state = sync_error(state);
                Some(message)
            },
            BackendMessage::CancelSync => {
                state = cancel_sync(state, &io_sender);
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
        println!("frontend_message: {:?}", frontend_message);
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
    let config = get_config(app);
    let frontend_state = FrontendState {
        message: frontend_message,
        update_all_metadata: config.should_update_all_metadata(),
        gps_coords: config.get_gps_coords(),
        output_dir: config.get_output_dir(app).to_str().unwrap().into(),
        backend_state: backend_state.clone(),
    };
    println!("frontend_state: {:?}", frontend_state);

    app.emit("update-state", frontend_state).unwrap();
}


/*** SYNC ***/

#[derive(Serialize, Deserialize, Clone)]
enum IOMessage {
    Sync,
    Cancel,
}

type IOSender = std::sync::mpsc::Sender<IOMessage>;
type IOReceiver = std::sync::mpsc::Receiver<IOMessage>;

#[derive(Clone)]
struct SyncItem {
    student: Student,
    timestamp: Zoned,
    url: reqwest::Url,
    object_id: String,
    extension: String,
}

fn run_sync_engine(app: AppHandle, output_root: PathBuf, bw_client: BrightwheelClient, io_receiver: IOReceiver, backend_sender: BackendSender) {
    let exiftool_path: PathBuf = app.path().resource_dir().unwrap().join("exiftool").join("exiftool");
    println!("exiftool_path: {}", exiftool_path.to_str().unwrap());
    let exif_tool = ExifTool::with_executable(&exiftool_path).unwrap();

    let mut io_service = IOService {
        output_root,
        app,
        bw_client,
        io_receiver,
        backend_sender,
        exif_tool,
        sync_index: 0,
        sync_items: Vec::new()
    };
    io_service.run();
}

struct IOService {
    output_root: PathBuf,
    app: AppHandle,
    bw_client: BrightwheelClient,
    io_receiver: IOReceiver,
    backend_sender: BackendSender,
    exif_tool: ExifTool,
    sync_index: usize,
    sync_items: Vec<SyncItem>,
}

impl IOService {
    const PAGE_SIZE: usize = 1000;

    fn run(&mut self) {
        loop {
            match self.sync_next_item() {
                Ok(synced) => {
                    if synced {
                        match self.io_receiver.try_recv() {
                            Ok(msg) => {
                                match msg {
                                    IOMessage::Sync => {
                                        println!("Should not receive sync message while downloading");
                                    },
                                    IOMessage::Cancel => {
                                        self.sync_index = 0;
                                        self.sync_items.clear();
                                        self.backend_sender.send(BackendMessage::SyncCanceled).unwrap();
                                    },
                                }
                            },
                            Err(e) => {
                                match e {
                                    std::sync::mpsc::TryRecvError::Empty => { },
                                    std::sync::mpsc::TryRecvError::Disconnected => {
                                        panic!("Should never get disconnected from IOService channel");
                                    },
                                }
                            },
                        }
                    }
                    else {
                        match self.io_receiver.recv().unwrap() {
                            IOMessage::Sync => {
                                if let Err(e) = self.sync() {
                                    self.sync_index = 0;
                                    self.sync_items.clear();
                                    self.backend_sender.send(BackendMessage::SyncError(
                                        format!("Received error syncing: {:?}", e))
                                    ).unwrap();
                                }
                            },
                            IOMessage::Cancel => {
                                println!("Nothing to cancel");
                            },
                        }
                    }
                },
                Err(e) => {
                    self.sync_index = 0;
                    self.sync_items.clear();
                    self.backend_sender.send(BackendMessage::SyncError(
                        format!("Received error syncing: {:?}", e))
                    ).unwrap();
                }
            }
        }
    }

    fn sync(&mut self) -> reqwest::Result<()> {
        // Get user_id
        let user_id = self.bw_client.get_user_id()?;
        println!("got user_id: {}", user_id);

        // Get list of students;
        let user_id_2 = user_id.clone();
        let students = self.bw_client.get_students(user_id_2)?;

        // Sync each student
        for student in students {
            self.sync_student(student)?;
        }

        Ok(())
    }

    fn sync_student(&mut self, student: Student) -> reqwest::Result<()> {
        println!("sync_student: {} {}", student.first_name, student.last_name);
        self.enqueue_sync_items(&student)?;

        println!("...done");

        Ok(())
    }

    fn enqueue_sync_items(&mut self, student: &Student) -> reqwest::Result<()> {
        println!("enqueue_sync_items: {} {}", student.first_name, student.last_name);

        let mut page: usize = 0;
        loop {
            self.backend_sender.send(BackendMessage::QueryingItems { page: page }).unwrap();
            let count = self.enqueue_sync_items_on_page(student, page)?;
            if count == 0 {
                break;
            }
            self.backend_sender.send(BackendMessage::QueriedItems {
                page: page,
                count: count,
            }).unwrap();
            page += 1;
        }

        Ok(())
    }

    fn enqueue_sync_items_on_page(&mut self, student: &Student, page: usize) -> reqwest::Result<usize> {
        let response = self.bw_client.get_students_activities(
            student.object_id.clone(), Self::PAGE_SIZE, page
        )?;
        let response_json = response.json::<Value>()?;
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
                self.sync_items.push(item);
                count += 1;
            }
        }

        Ok(count)
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

    fn sync_next_item(&mut self) -> reqwest::Result<bool> {
        // println!("todo: download {} {} {} {}.{}", item.student.first_name, item.student.last_name, item.timestamp, item.object_id, item.extension);

        if self.sync_index < self.sync_items.len() {
            let item = &self.sync_items[self.sync_index];

            let student_path = create_student_path(&self.output_root, &item.student);
            let month_path = create_month_path(&student_path, &item.timestamp);
            let filename = format_filename(&item.timestamp, &item.object_id,  &item.extension);
            let dst_path = month_path.join(filename.clone());

            println!("{:?}", dst_path);
            let needs_download = !dst_path.exists();
            self.backend_sender.send(BackendMessage::ProcessingItem {
                needs_download,
                path: dst_path.clone(),
                index: self.sync_index,
                count: self.sync_items.len(),
            }).unwrap();
            if needs_download {
                let dst_path_tmp = temp_dir(&self.app).join(filename);
                self.bw_client.download_file(&item.url, &dst_path_tmp)?;
                println!("Renaming {:?} to {:?}", dst_path_tmp, dst_path);
                fs::copy(dst_path_tmp.clone(), dst_path.clone()).unwrap();
                fs::remove_file(dst_path_tmp).unwrap();

                // Modify system creation/modification time
                let _ = fs::File::open(&dst_path).unwrap().set_modified(
                    SystemTime::UNIX_EPOCH + Duration::from_nanos(item.timestamp.timestamp().as_nanosecond().try_into().unwrap())
                );
            }

            if needs_download || should_update_all_metadata(&self.app) {
                // Add GPS coordinates and date/time to metadata
                let output: Vec<String> = self.exif_tool.execute_lines(&[
                    "-overwrite_original", "-alldates<filename",
                    &format!("-gpsposition={}", get_gps_coords(&self.app)),
                    dst_path.to_str().unwrap()
                ]).unwrap();
                for line in output {
                    println!("{}", line);
                }
            }

            self.sync_index += 1;
            if self.sync_index == self.sync_items.len() {
                self.sync_index = 0;
                self.sync_items.clear();
                self.backend_sender.send(BackendMessage::SyncComplete).unwrap();
            }

            Ok(true)
        }
        else {
            Ok(false)
        }
    }

    fn log_to_frontend(&self, msg: String) {
        self.backend_sender.send(BackendMessage::LogToFrontend(msg)).unwrap();
    }
}

fn sync(state: BackendState, io_sender: &IOSender) -> BackendState {
    match state {
        BackendState::LoggedIn(logged_in_state) => {
            logged_in_state.sync(io_sender)
        },
        _ => {
            println!("Unexpected state for sync: {:?}", state);
            state
        }
    }
}

fn cancel_sync(state: BackendState, io_sender: &IOSender) -> BackendState {
    match state {
        BackendState::Syncing(syncing_state) => {
            syncing_state.cancel_sync(io_sender)
        },
        _ => {
            println!("Unexpected state for cancel sync: {:?}", state);
            state
        }
    }
}

fn sync_canceled(state: BackendState) -> BackendState {
    match state {
        BackendState::SyncCanceling(sync_canceling_state) => {
            sync_canceling_state.sync_canceled()
        },
        _ => {
            println!("Unexpected state for sync canceled: {:?}", state);
            state
        }
    }
}

fn sync_complete(state: BackendState) -> BackendState {
    match state {
        BackendState::Syncing(syncing_state) => {
            syncing_state.sync_complete()
        },
        _ => {
            state
        }
    }
}

fn sync_error(state: BackendState) -> BackendState {
    match state {
        BackendState::Syncing(syncing_state) => {
            syncing_state.sync_error()
        },
        _ => {
            println!("Unexpected state for sync error: {:?}", state);
            state
        }
    }
}

fn update_login_state_from_cookies(app: &AppHandle, state: BackendState, cookies: &Vec<Cookie>) -> BackendState {
    let mut logged_in = false;
    for cookie in cookies {
        println!("cookie: {:?}", cookie);
        if cookie.name().eq_ignore_ascii_case("_brightwheel_v2") {
            logged_in = true;
        }
    }

    match state {
        BackendState::LoggedOut(logged_out_state) => {
            BackendState::LoggedIn(LoggedInState { })
        },
        _ => state
    }
}

impl LoggedOutState {
    fn log_in(self) -> BackendState {
        BackendState::LoggedIn(LoggedInState {  })
    }
}

impl LoggedInState {
    fn sync(self, io_sender: &IOSender) -> BackendState {
        io_sender.send(IOMessage::Sync).unwrap();
        BackendState::Syncing(SyncingState { })
    }
}

impl SyncingState {
    fn cancel_sync(self, io_sender: &IOSender) -> BackendState {
        io_sender.send(IOMessage::Cancel).unwrap();
        BackendState::SyncCanceling(SyncCancelingState { })
    }

    fn sync_complete(self) -> BackendState {
        BackendState::LoggedIn(LoggedInState { })
    }

    fn sync_error(self) -> BackendState {
        BackendState::LoggedIn(LoggedInState { })
    }
}

impl SyncCancelingState {
    fn sync_canceled(self) -> BackendState {
        BackendState::LoggedIn(LoggedInState { })
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

fn temp_dir(app: &AppHandle) -> PathBuf {
    app.path().temp_dir().unwrap()
}


/*** UTILITY FUNCTIONS ***/

fn config_dir(app: &AppHandle) -> PathBuf {
    app.path().app_config_dir().unwrap()
}

fn config_path(app: &AppHandle) -> PathBuf {
    config_dir(app).join("config.json")
}

fn get_config(app: &AppHandle) -> Config {
    let config_path = config_path(app);
    if config_path.exists() {
        let file = std::fs::File::open(&config_path).unwrap();
        serde_json::from_reader(file).unwrap()
    }
    else {
        Config {
            output_dir: None,
            update_all_metadata: None,
            gps_coords: None,
        }
    }
}

fn write_config(app: &AppHandle, config: Config) {
    let config_path = config_path(app);
    println!("config_path: {:?}", config_path);
    let file = std::fs::File::options()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&config_path).unwrap();
    serde_json::to_writer(file, &config).unwrap();
}

fn set_output_dir(app: &AppHandle, output_dir: PathBuf) {
    let mut config = get_config(app);
    config.output_dir = Some(output_dir);
    write_config(app, config);
}

fn set_update_all_metadata(app: &AppHandle, update_all_metdata: bool) {
    let mut config = get_config(app);
    config.update_all_metadata = Some(update_all_metdata);
    write_config(app, config);
}

fn set_gps_coords(app: &AppHandle, gps_coords: String) {
    let mut config = get_config(app);
    config.gps_coords = Some(gps_coords);
    write_config(app, config);
}

fn cookies_path(app: &AppHandle) -> PathBuf {
    app.path().app_config_dir().unwrap().join("cookies.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    output_dir: Option<PathBuf>,
    update_all_metadata: Option<bool>,
    gps_coords: Option<String>,
}

impl Config {
    fn get_output_dir(&self, app: &AppHandle) -> PathBuf {
        self.output_dir.clone().unwrap_or(
            app.path().picture_dir().unwrap().join("shinydisc")
        )
    }

    fn should_update_all_metadata(&self) -> bool {
        self.update_all_metadata.unwrap_or(false)
    }

    fn get_gps_coords(&self) -> String {
        self.gps_coords.clone().unwrap_or("37.78401, -122.50331".into())
    }
}

fn should_update_all_metadata(app: &AppHandle) -> bool {
    get_config(app).should_update_all_metadata()
}

fn get_gps_coords(app: &AppHandle) -> String {
    get_config(app).get_gps_coords()
}

fn get_output_dir(app: &AppHandle) -> PathBuf {
    get_config(app).get_output_dir(app)
}

fn remove_cookies(app: &AppHandle, cookie_store_arc_mutex: &Arc<CookieStoreMutex>) {
    cookie_store_arc_mutex.lock().unwrap().clear();
    delete_cookies(app);
}

#[allow(deprecated)]
fn delete_cookies(app: &AppHandle) {
    let path = cookies_path(app);
    if path.exists() {
        std::fs::remove_file(path).unwrap();
    }
}

#[allow(deprecated)]
fn write_cookies(app: &AppHandle, cookie_store_arc_mutex: &Arc<CookieStoreMutex>) {
    let mut writer = std::fs::File::create(cookies_path(app))
      .map(std::io::BufWriter::new)
      .unwrap();
    cookie_store_arc_mutex.lock().unwrap().save_json(&mut writer).unwrap();
}

#[allow(deprecated)]
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
