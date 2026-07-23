
mod brightwheel;
mod ioservice;
mod config;

use std::{path::PathBuf, str::FromStr, sync::Arc};

use jiff::Zoned;
use reqwest_cookie_store::CookieStoreMutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Builder, Emitter, Manager, Url};

use exiftool::ExifTool;

use crate::brightwheel::*;
use crate::ioservice::*;
use crate::config::*;

type BackendReceiver = std::sync::mpsc::Receiver<BackendMessage>;
type BackendSender = std::sync::mpsc::Sender<BackendMessage>;

#[derive(Serialize, Deserialize, Debug, Clone)]
enum BackendState {
    LoggedOut(LoggedOutState),
    LoggingIn,
    LoggedIn(LoggedInState),
    SyncQuerying,
    Syncing(SyncingState),
    SyncCanceling(SyncCancelingState),
    Error,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct LoggedOutState {
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct LoggedInState {
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SyncingState {
    sync_index: usize,
    sync_items: Vec<ItemToSync>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct SyncCancelingState {

}

#[derive(Serialize, Deserialize, Clone)]
enum BackendMessage {
    Test,
    Error,
    TryAgain,
    LoginTestFinished(bool),
    IndexDOMContentLoaded,
    SetOutputDir(String),
    SetUpdateAllMetadata(bool),
    SetGPSCoords(String),
    Sync,
    LogOut,
    GotAllSyncItems(Vec<ItemToSync>),
    SyncedItem(PathBuf),
    // QueryingItems {
    //     page: usize,
    // },
    // QueriedItems {
    //     page: usize,
    //     count: usize,
    // },
    // ProcessingItem {
    //     needs_download: bool,
    //     path: PathBuf,
    //     index: usize,
    //     count: usize,
    // },
    // SyncComplete,
    // SyncError(String),
    CancelSync,
    // SyncCanceled,
    // LogToFrontend(String),
    Exit
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (backend_sender, backend_receiver) = std::sync::mpsc::channel();
    let backend_sender_app_manage = backend_sender.clone();
    // let backend_sender_on_navigation = backend_sender.clone();

    let app = Builder::default()
        .setup(|app| {
            app.manage(backend_sender_app_manage);
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![send_backend_message, get_config_tauri])
        .build(
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

pub fn clear_cookies(app: &AppHandle, cookie_store_arc_mutex: &Arc<CookieStoreMutex>) {
    app.get_webview_window("main").unwrap().clear_all_browsing_data().unwrap();
    let mut guard = cookie_store_arc_mutex.lock().unwrap();
    guard.clear();
}

pub fn update_cookies(app: &AppHandle, cookie_store_arc_mutex: &Arc<CookieStoreMutex>) {
    let cookies = app.get_webview_window("main").unwrap().cookies().unwrap().to_owned();
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
    let bw_client = BrightwheelClient::new(cookie_store_arc_mutex.clone());
    let mut state = BackendState::LoggedOut(LoggedOutState { });

    // Launch IO thread (thus avoiding the horrors of Rust async)
    let (io_sender, io_receiver) = std::sync::mpsc::channel();
    {
        let app_2 = app.clone();
        let output_root = get_output_dir(&app_2);
        std::thread::spawn(move || {
            run_sync_engine(app_2, output_root, bw_client, io_receiver, sender);
        });
    }

    let mut base_url_opt: Option<Url> = None;

    loop {
        let frontend_message = match receiver.recv().unwrap() {
            BackendMessage::Test => {
                respond_to_test_message(&app);
                Some("Test message".to_string())
            },
            BackendMessage::Error => {
                go_to_error_page(&app, &base_url_opt);
                state = BackendState::Error;
                None
            },
            BackendMessage::TryAgain => {
                state = BackendState::LoggedOut(LoggedOutState {});
                restart(&app, &base_url_opt);
                None
            },
            BackendMessage::LoginTestFinished(logged_in) => {
                update_cookies(&app, &cookie_store_arc_mutex);
                if logged_in {
                    state = match state {
                        BackendState::LoggedOut(_) => {
                            log_in(&app, &base_url_opt);
                            BackendState::LoggedIn(LoggedInState {  })
                        },
                        BackendState::LoggingIn => {
                            log_in(&app, &base_url_opt);
                            BackendState::LoggedIn(LoggedInState { })
                        },
                        _ => state,
                    };
                }
                else {
                    state = match state {
                        BackendState::LoggedOut(_) => {
                            // This message comes from the initial login test.
                            // Need to initiate login.
                            initiate_login(&app, &io_sender);
                            BackendState::LoggingIn
                        },
                        BackendState::LoggingIn => {
                            // If we were already in the logging-in state, resubmit the login test after a delay
                            // until we're logged in.
                            io_sender.send(IOMessage::Sleep(2.0)).unwrap();
                            io_sender.send(IOMessage::TestLogin).unwrap();
                            BackendState::LoggingIn
                        },
                        _ => {
                            panic!("Got login test result in nonsensical state. This is a logic error.");
                        },
                    }
                }
                None
            },
            BackendMessage::IndexDOMContentLoaded => {
                if base_url_opt.is_none() {
                    base_url_opt = Some(app.get_webview_window("main").unwrap().url().unwrap());

                }
                update_cookies(&app, &cookie_store_arc_mutex);
                io_sender.send(IOMessage::TestLogin).unwrap();
                println!("received notification of DOMContentLoaded on backend");
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
                state = sync(&app, &base_url_opt, state, &io_sender);
                None
            },
            BackendMessage::LogOut => {
                clear_cookies(&app, &cookie_store_arc_mutex);
                state = BackendState::LoggingIn;
                initiate_login(&app, &io_sender);
                None
            },
            BackendMessage::GotAllSyncItems(sync_items) => {
                println!("Got {} sync items", sync_items.len());
                if sync_items.len() > 0 {
                    state = match state {
                        BackendState::SyncQuerying => {
                            show_syncing_message(&app, "Processing items...");
                            io_sender.send(IOMessage::SyncItem(sync_items[0].clone())).unwrap();
                            BackendState::Syncing(
                                SyncingState {
                                    sync_index: 0,
                                    sync_items: sync_items,
                                }
                            )

                        },
                        _ => state
                    };
                }
                else {
                    state = match state {
                        BackendState::SyncQuerying => {
                            navigate_to_local_path(&app, &base_url_opt, "loggedin.html");
                            BackendState::LoggedIn(LoggedInState { })
                        },
                        _ => state
                    }
                }
                None
            },
            BackendMessage::SyncedItem(path) => {
                state = match state {
                    BackendState::Syncing(syncing_state) => {
                        let next_index = syncing_state.sync_index + 1;
                        show_syncing_message(&app, &format!("Processed {}/{}:<br>{}",
                            next_index, syncing_state.sync_items.len(), path.to_str().unwrap()
                        ));
                        if next_index < syncing_state.sync_items.len() {
                            io_sender.send(IOMessage::Sleep(0.001)).unwrap();
                            io_sender.send(IOMessage::SyncItem(syncing_state.sync_items[next_index].clone())).unwrap();
                            BackendState::Syncing(
                                SyncingState {
                                    sync_index: next_index,
                                    sync_items: syncing_state.sync_items,
                                }
                            )
                        }
                        else {
                            navigate_to_local_path(&app, &base_url_opt, "loggedin.html");
                            BackendState::LoggedIn(LoggedInState { })
                        }
                    },
                    _ => state
                };
                None
            },
            // BackendMessage::QueryingItems { page } => {
            //     Some(format!("Querying items from page {}", page + 1))
            // },
            // BackendMessage::QueriedItems { page, count } => {
            //     Some(format!("Query found {} items on page {}", count, page + 1))
            // },
            // BackendMessage::ProcessingItem {
            //     needs_download,
            //     path,
            //     index,
            //     count,
            // } => {
            //     let base_msg = format!("{} ({}/{})", path.to_str().unwrap().to_string(), index + 1, count);
            //     if needs_download {
            //         Some(base_msg)
            //     }
            //     else {
            //         Some(format!("{} - already downloaded", base_msg))
            //     }
            // },
            // BackendMessage::SyncComplete => {
            //     state = sync_complete(state);
            //     Some("Sync complete".to_string())
            // },
            // BackendMessage::SyncError(message) => {
            //     state = sync_error(state);
            //     Some(message)
            // },
            BackendMessage::CancelSync => {
                state = cancel_sync(state, &io_sender);
                Some("Cancelling...".to_string())
            },
            // BackendMessage::SyncCanceled => {
            //     state = sync_canceled(state);
            //     Some("Sync canceled.".to_string())
            // },
            // BackendMessage::LogToFrontend(msg) => {
            //     log_to_frontend(&app, msg);
            //     None
            // }
            BackendMessage::Exit => {
                break;
            }
        };
    }
}


/*** ALL-PURPOSE BACKEND MESSAGING COMMAND ***/

#[tauri::command]
fn send_backend_message(sender: tauri::State<'_, BackendSender>, message: BackendMessage) -> Result<(), String> {
  match sender.send(message) {
    Ok(()) => Ok(()),
    Err(e) => {
      println!("Got error sending backend message: {:?}", e);
      panic!()
    }
  }
}

#[tauri::command]
fn get_config_tauri(app: AppHandle) -> Config {
    get_config(&app)
}

/*** NAVIGATION ***/

fn restart(app: &AppHandle, base_url_opt: &Option<Url>) {
    navigate_to_local_path(&app, &base_url_opt, "index.html");
}

fn go_to_error_page(app: &AppHandle, base_url_opt: &Option<Url>) {
    navigate_to_local_path(&app, &base_url_opt, "error.html");
}

fn log_in(app: &AppHandle, base_url_opt: &Option<Url>) {
    navigate_to_local_path(&app, &base_url_opt, "loggedin.html");
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

/*** FRONTEND MESSAGES FOR USER ***/

fn show_syncing_message(app: &AppHandle, message: &str) {
    app.emit("sync-message", message).unwrap();
}

/*** INITIATE LOGIN VIA BRIGHTWHEEL ***/

fn initiate_login(app: &AppHandle, io_sender: &IOSender) {
    let wvw = app.get_webview_window("main").unwrap();
    wvw.navigate(Url::parse("https://schools.mybrightwheel.com/").unwrap()).unwrap();
    io_sender.send(IOMessage::Sleep(5.0)).unwrap();
    io_sender.send(IOMessage::TestLogin).unwrap();
}

/*** FRONTEND STATE UPDATE ***/

fn navigate_to_local_path(app: &AppHandle, base_url_opt: &Option<Url>, path_str: &str) {
    if let Some(base_url) = base_url_opt {
        let wvw = app.get_webview_window("main").unwrap();
        wvw.navigate(base_url.join(path_str).unwrap()).unwrap();
    }
    else {
        panic!("No base URL to navigate from");
    }
}


/*** SYNC ***/

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ItemToSync {
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
    };
    io_service.run();
}

fn sync(app: &AppHandle, base_url_opt: &Option<Url>, state: BackendState, io_sender: &IOSender) -> BackendState {
    match state {
        BackendState::LoggedIn(logged_in_state) => {
            navigate_to_local_path(app, base_url_opt, "syncing.html");
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

impl LoggedOutState {
    fn log_in(self) -> BackendState {
        BackendState::LoggedIn(LoggedInState {  })
    }
}

impl LoggedInState {
    fn sync(self, io_sender: &IOSender) -> BackendState {
        io_sender.send(IOMessage::GetAllSyncItems).unwrap();
        BackendState::SyncQuerying
    }
}

impl SyncingState {
    fn cancel_sync(self, io_sender: &IOSender) -> BackendState {
        // io_sender.send(IOMessage::Cancel).unwrap();
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

