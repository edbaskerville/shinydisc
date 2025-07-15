use std::collections::HashMap;

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
    header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE, ORIGIN, REFERER, USER_AGENT},
    blocking::Client
};

pub struct Brightwheel {
    client: Client,
    auth_headers: HeaderMap,
}

impl Brightwheel {
    pub fn new() -> Self {
        let client = Client::builder()
            .cookie_store(true).build().unwrap();
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
            auth_headers,
        }
    }

    pub fn authenticate_email_password(&self, email: &str, password: &str) {
        let request = self.client.post(
            format!("{}/sessions/start", URL_BASE)
        )
            .headers(self.auth_headers.clone())
            .json(&Self::authentication_map(email, password, None))
            .build().unwrap();
        let response = self.client.execute(request).unwrap();
        println!("{}", response.text().unwrap());
    }

    pub fn authenticate_mfa(&self, email: &str, password: &str, mfa_code_opt: Option<&str>) {
        let request = self.client.post(
            format!("{}/sessions/start", URL_BASE)
        )
            .headers(self.auth_headers.clone())
            .json(&Self::authentication_map(email, password, mfa_code_opt))
            .build().unwrap();
        self.client.execute(request).unwrap();
    }

    fn authentication_map(email: &str, password: &str, mfa_code_opt: Option<&str>) -> HashMap<String, HashMap<String, String>> {
        let mut map = hash_map! {
            "email".to_string() => email.to_string(),
            "password".to_string() => password.to_string(),
        };
        if let Some(mfa_code) = mfa_code_opt {
            map.insert("2fa_code".to_string(), mfa_code.to_string());
        }

        hash_map!{
            "user".to_string() => map
        }
    }
}
