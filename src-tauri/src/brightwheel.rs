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
use serde_json::{json, Value};

pub struct BrightwheelClient {
    client: Client,
    pub cookie_store_arc_mutex: Arc<CookieStoreMutex>,
    auth_headers: HeaderMap,
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
