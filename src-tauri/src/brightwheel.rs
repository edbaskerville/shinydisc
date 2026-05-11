use std::{path::PathBuf, sync::{Arc}, time::Duration};

const URL_BASE: &str = "https://schools.mybrightwheel.com/api/v1";

use reqwest::{
    blocking::{Client, Response}, header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE, ORIGIN, REFERER, USER_AGENT}
};
use reqwest_cookie_store::CookieStoreMutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub struct BrightwheelClient {
    client: Client,
    pub cookie_store_arc_mutex: Arc<CookieStoreMutex>,
    auth_headers: HeaderMap,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Student {
    pub object_id: String,
    pub first_name: String,
    pub last_name: String,
}

impl BrightwheelClient {
    fn make_client(cookie_store_arc_mutex: Arc<CookieStoreMutex>) -> Client{
        Client::builder().cookie_provider(cookie_store_arc_mutex).timeout(Duration::from_secs(30)).build().unwrap()
    }

    pub fn new(cookie_store: reqwest_cookie_store::CookieStore) -> Self {
        let cookie_store_arc_mutex = Arc::new(
            CookieStoreMutex::new(cookie_store)
        );

        let client = Self::make_client(cookie_store_arc_mutex.clone());
        let auth_headers = HeaderMap::from_iter(vec![
            (CONTENT_TYPE, HeaderValue::from_str("application/json").unwrap()),
            (
                HeaderName::from_static("x-client-version"), 
                HeaderValue::from_str("106").unwrap(),
            ),
            (
                HeaderName::from_static("x-client-name"),
                HeaderValue::from_str("web").unwrap(),
            ),
            (ORIGIN, HeaderValue::from_str("https://schools.mybrightwheel.com").unwrap()),
            (REFERER, HeaderValue::from_str("https://schools.mybrightwheel.com/sign-in").unwrap()),
            // (USER_AGENT, HeaderValue::from_str("Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:139.0) Gecko/20100101 Firefox/139.0").unwrap()),
            (USER_AGENT, HeaderValue::from_str("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.7727.56 Safari/537.36").unwrap()),
        ].into_iter());

        Self {
            client,
            cookie_store_arc_mutex,
            auth_headers,
        }
    }

    // pub fn post_sessions_start(&self, email: String, password: String) -> reqwest::Result<Response> {
    //     let request = self.client.post(
    //         format!("{}/sessions/start", URL_BASE)
    //     )
    //         .headers(self.auth_headers.clone())
    //         .json(&Self::authentication_json(email, password, None))
    //         .build().unwrap();
    //     self.client.execute(request)
    // }

    // pub fn post_sessions(&self, email: String, password: String, mfa_code_opt: Option<String>) -> reqwest::Result<Response> {
    //     let request = self.client.post(
    //         format!("{}/sessions", URL_BASE)
    //     )
    //         .headers(self.auth_headers.clone())
    //         .json(&Self::authentication_json(email, password, mfa_code_opt))
    //         .build().unwrap();
    //     self.client.execute(request)
    // }

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

//     fn authentication_json(email: String, password: String, mfa_code_opt: Option<String>) -> Value {
//         let mut json_val = json!({
//             "user" : {
//                 "email" : email,
//                 "password" : password
//             }
//         });
//         
//         if let Some(mfa_code) = mfa_code_opt {
//             json_val.as_object_mut().unwrap().insert("2fa_code".into(), mfa_code.into());
//         }
//         json_val
//     }

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

impl Clone for BrightwheelClient {
    fn clone(&self) -> Self {
        Self {
            client: Self::make_client(self.cookie_store_arc_mutex.clone()),
            cookie_store_arc_mutex: self.cookie_store_arc_mutex.clone(),
            auth_headers: self.auth_headers.clone()
        }
    }
}
