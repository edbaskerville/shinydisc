use std::{path::PathBuf, sync::{Arc, Mutex}, time::Duration};

const URL_BASE: &str = "https://schools.mybrightwheel.com/api/v1";

use reqwest::{
    blocking::{Client, Response}, cookie::CookieStore, header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, ORIGIN, REFERER, USER_AGENT}
};
use tauri::{Url, webview};
use reqwest_cookie_store::{CookieStoreMutex, RawCookieParseError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub struct BrightwheelClient {
    client: Client,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Student {
    pub object_id: String,
    pub first_name: String,
    pub last_name: String,
}

impl BrightwheelClient {
    pub fn new(cookie_store_arc_mutex: Arc<CookieStoreMutex>) -> Self {
        let client = Client::builder()
            .cookie_provider(cookie_store_arc_mutex)
            .timeout(Duration::from_secs(30))
            .build().unwrap();

        Self {
            client,
        }
    }

    pub fn get_users_me(&self) -> reqwest::Result<Response> {
        let request = self.client.get(format!("{}/users/me", URL_BASE)).build().unwrap();
        self.client.execute(request)
    }

    pub fn get_user_id(&self) -> reqwest::Result<String> {
        let response: Response = self.get_users_me()?;
        let json = response.json::<Value>().unwrap();
        println!("users/me json: {:?}", json);
        match json {
            Value::Object(obj) => {
                match obj.get("object_id").unwrap() {
                    Value::String(user_id) => Ok(user_id.clone()),
                    _ => panic!()
                }
            },
            _ => panic!()
        }
    }

    pub fn get_guardians_students(&self, user_id: String) -> reqwest::Result<Response> {
        let request = self.client.get(format!("{}/guardians/{}/students", URL_BASE, user_id)).build().unwrap();
        self.client.execute(request)
    }

    pub fn get_students(&self, user_id: String) -> reqwest::Result<Vec<Student>> {
        // TODO: handle parse errors

        let response = self.get_guardians_students(user_id.clone())?;
        let json = response.json::<Value>().unwrap();
        println!("guardians/{}/students json: {:?}", user_id, json);

        Ok(Vec::from_iter(
            match &json {
                Value::Object(obj) => {
                    match obj.get("students").unwrap() {
                        Value::Array(arr) => {
                            arr.iter().map(|item| {
                                match item {
                                    Value::Object(item_obj) => {
                                        let student_val = item_obj.get("student").unwrap();
                                        match student_val {
                                            Value::Object(student_obj) => {
                                                let object_id = student_obj.get("object_id").unwrap().as_str().unwrap().into();
                                                let first_name = student_obj.get("first_name").unwrap().as_str().unwrap().into();
                                                let last_name = student_obj.get("last_name").unwrap().as_str().unwrap().into();
                                                Student {
                                                    object_id,
                                                    first_name,
                                                    last_name,
                                                }
                                            },
                                            _ => panic!()
                                        }
                                    },
                                    _ => panic!()
                                }
                            })
                        },
                        _ => panic!()
                    }
                },
                _ => panic!()
            }
        ))
    }

    pub fn get_students_activities(&self, student_id: String, page_size: usize, page: usize) -> reqwest::Result<Response> {
        let request = self.client.get(
            format!("{}/students/{}/activities", URL_BASE, student_id)
        ).query(
            &[("page_size", page_size), ("page", page)]
        ).build().unwrap();
        self.client.execute(request)
    }

    pub fn download_file(&self, src_url: &reqwest::Url, dst_path: &PathBuf) -> reqwest::Result<()> {
        let request = self.client.get(
            src_url.clone()
        ).build().unwrap();
        let mut file: std::fs::File = std::fs::File::create(dst_path).unwrap();
        let mut response = self.client.execute(request)?;
        response.copy_to(&mut file)?;

        Ok(())
    }
}
