use std::{collections::HashMap, io::BufRead, sync::{Arc, Mutex}};

use map_macro::hash_map;

const URL_BASE: &str = "https://schools.mybrightwheel.com/api/v1/";

const COOKIE_NAME: &str = "_brightwheel_v2";
const COOKIE_DOMAIN: &str = ".mybrightwheel.com";


const AUTH_HEADERS_JSON: &str = r#"{
    "Content-Type": "application/json",
    "X-Client-Version": "106",
    "X-Client-Name": "web",
    "Origin": "https://schools.mybrightwheel.com",
    "Referer": "https://schools.mybrightwheel.com/sign-in",
    "User-Agent": "Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:139.0) Gecko/20100101 Firefox/139.0"
}"#;

use reqwest::{
    blocking::{Client, Response}, cookie::{Jar}, header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE, ORIGIN, REFERER, USER_AGENT}
};
use reqwest_cookie_store::CookieStoreMutex;
use serde::Serialize;
use serde_json::{json, Value};

pub struct BrightwheelClient {
    client: Client,
    pub cookie_store_arc_mutex: Arc<CookieStoreMutex>,
    auth_headers: HeaderMap,
}

#[derive(Serialize, Debug)]
pub struct Student {
    pub object_id: String,
    pub first_name: String,
    pub last_name: String,
}

impl BrightwheelClient {
    pub fn new(cookie_store: reqwest_cookie_store::CookieStore) -> Self {
        let cookie_store_arc_mutex = Arc::new(
            CookieStoreMutex::new(cookie_store)
        );

        let client = Client::builder()
            .cookie_provider(std::sync::Arc::clone(&cookie_store_arc_mutex))
            .build().unwrap();
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
            (USER_AGENT, HeaderValue::from_str("Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:139.0) Gecko/20100101 Firefox/139.0").unwrap()),
        ].into_iter());

        Self {
            client,
            cookie_store_arc_mutex,
            auth_headers,
        }
    }

    pub fn post_sessions_start(&self, email: &str, password: &str) -> Response {
        let request = self.client.post(
            format!("{}/sessions/start", URL_BASE)
        )
            .headers(self.auth_headers.clone())
            .json(&Self::authentication_json(email, password, None))
            .build().unwrap();
        self.client.execute(request).unwrap()
    }

    pub fn post_sessions(&self, email: &str, password: &str, mfa_code_opt: Option<&str>) -> Response {
        let request = self.client.post(
            format!("{}/sessions", URL_BASE)
        )
            .headers(self.auth_headers.clone())
            .json(&Self::authentication_json(email, password, mfa_code_opt))
            .build().unwrap();
        self.client.execute(request).unwrap()
    }

    pub fn get_users_me(&self) -> Response {
        let request = self.client.get(format!("{}/users/me", URL_BASE)).build().unwrap();
        self.client.execute(request).unwrap()
    }

    pub fn get_user_id(&self) -> String {
        let response = self.get_users_me();
        let json = response.json::<Value>().unwrap();
        println!("users/me json: {:?}", json);
        match json {
            Value::Object(obj) => {
                match obj.get("object_id").unwrap() {
                    Value::String(user_id) => user_id.clone(),
                    _ => panic!()
                }
            },
            _ => panic!()
        }
    }

    pub fn get_guardians_students(&self, user_id: &String) -> Response {
        let request = self.client.get(format!("{}/guardians/{}/students", URL_BASE, user_id)).build().unwrap();
        self.client.execute(request).unwrap()
    }

    pub fn get_students(&self, user_id: &String) -> Vec<Student> {
        let response = self.get_guardians_students(user_id);
        let json = response.json::<Value>().unwrap();
        println!("guardians/{}/students json: {:?}", user_id, json);

        Vec::from_iter(
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
        )
    }

    pub fn get_students_activities(&self, student_id: &String) -> Response {
        let request = self.client.get(
            format!("{}/students/{}/activities", URL_BASE, student_id)
        ).query(
            &[("page_size", 10), ("offset", 0)]
        ).build().unwrap();
        self.client.execute(request).unwrap()
    }

    fn authentication_json(email: &str, password: &str, mfa_code_opt: Option<&str>) -> Value {
        let mut json_val = json!({
            "user" : {
                "email" : email,
                "password" : password
            }
        });
        
        if let Some(mfa_code) = mfa_code_opt {
            json_val.as_object_mut().unwrap().insert("2fa_code".into(), mfa_code.into());
        }
        json_val
    }
}
