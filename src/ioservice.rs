use std::{fs, path::PathBuf, str::FromStr, sync::Arc, time::{Duration, SystemTime}};

use exiftool::ExifTool;
use jiff::{Timestamp, Zoned};
use serde::{Serialize, Deserialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager};
use crate::{BackendMessage, BackendSender, ItemToSync, brightwheel::{BrightwheelClient, Student}, get_gps_coords, should_update_all_metadata};

#[derive(Serialize, Deserialize, Clone)]
pub enum IOMessage {
    // Sync,
    // Cancel,
    Sleep(f64),
    TestLogin,
    GetAllSyncItems,
    SyncItem(ItemToSync),
}

pub type IOSender = std::sync::mpsc::Sender<IOMessage>;
pub type IOReceiver = std::sync::mpsc::Receiver<IOMessage>;

pub struct IOService {
    pub output_root: PathBuf,
    pub app: AppHandle,
    pub bw_client: BrightwheelClient,
    pub io_receiver: IOReceiver,
    pub backend_sender: BackendSender,
    pub exif_tool: ExifTool,
}

impl IOService {
    const PAGE_SIZE: usize = 1000;

    pub fn run(&mut self) {
        loop {
            match self.io_receiver.recv().unwrap() {
                IOMessage::Sleep(secs) => {
                    std::thread::sleep(Duration::from_secs_f64(secs));
                },
                IOMessage::TestLogin => {
                    match self.bw_client.get_login_test() {
                        Ok(logged_in) => {
                            println!("Logged In? {:?}", logged_in);
                            self.backend_sender.send(BackendMessage::LoginTestFinished(logged_in)).unwrap();
                        },
                        Err(e) => {
                            println!("get_login_test() error: {:?}", e);
                        }
                    }
                },
                IOMessage::GetAllSyncItems => {
                    let sync_items = self.get_all_sync_items().unwrap();
                    self.backend_sender.send(BackendMessage::GotAllSyncItems(sync_items));
                },
                IOMessage::SyncItem(item) => {
                    self.sync_item(&item);
                    self.backend_sender.send(BackendMessage::SyncedItem(item)).unwrap();
                }
            }
        }
    }

    fn get_all_sync_items(&mut self) -> reqwest::Result<Vec<ItemToSync>> {
        let mut sync_items = Vec::new();

        // Get user_id
        let user_id = self.bw_client.get_user_id()?;
        println!("got user_id: {}", user_id);

        // Get list of students;
        let user_id_2 = user_id.clone();
        let students = self.bw_client.get_students(user_id_2)?;

        // Sync each student
        for student in students {
            let mut sync_items_for_student = self.get_sync_items_for_student(&student)?;
            sync_items.append(&mut sync_items_for_student);
        }

        Ok(sync_items)
    }

    fn get_sync_items_for_student(&mut self, student: &Student) -> reqwest::Result<Vec<ItemToSync>> {
        println!("enqueue_sync_items: {} {}", student.first_name, student.last_name);

        let mut sync_items = Vec::new();
        let mut page: usize = 0;
        loop {
            // self.backend_sender.send(BackendMessage::QueryingItems { page: page }).unwrap();
            let mut sync_items_for_page = self.get_sync_items_for_page(student, page)?;
            let count = sync_items_for_page.len();
            if count == 0 {
                break;
            }

            sync_items.append(&mut sync_items_for_page);
            // self.backend_sender.send(BackendMessage::QueriedItems {
            //     page: page,
            //     count: count,
            // }).unwrap();
            page += 1;
        }

        Ok(sync_items)
    }


    fn get_sync_items_for_page(&mut self, student: &Student, page: usize) -> reqwest::Result<Vec<ItemToSync>> {
        let mut sync_items = Vec::new();

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

        for activity in activities {
            if let Some(item) = self.get_sync_item_for_activity(student, activity.as_object().unwrap()) {
                sync_items.push(item);
            }
        }

        Ok(sync_items)
    }


    fn get_sync_item_for_activity(&mut self, student: &Student, activity: &Map<String, Value>) -> Option<ItemToSync> {
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
            ItemToSync {
                student: student.clone(),
                        timestamp,
                        url,
                        object_id,
                        extension,
            }
        })
    }

    fn sync_item(&mut self, item: &ItemToSync) -> reqwest::Result<()> {
        println!("download {} {} {} {}.{}", item.student.first_name, item.student.last_name, item.timestamp, item.object_id, item.extension);

        let student_path = create_student_path(&self.output_root, &item.student);
        let month_path = create_month_path(&student_path, &item.timestamp);
        let filename = format_filename(&item.timestamp, &item.object_id,  &item.extension);
        let dst_path = month_path.join(filename.clone());

        println!("{:?}", dst_path);
        let needs_download = !dst_path.exists();
        // self.backend_sender.send(BackendMessage::ProcessingItem {
        //     needs_download,
        //     path: dst_path.clone(),
        //     index: self.sync_index,
        //     count: self.sync_items.len(),
        // }).unwrap();
        if needs_download {
            let dst_path_tmp = temp_dir(&self.app).join(filename);
            println!("Downloading to {:?}...", dst_path_tmp);
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

        Ok(())
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
