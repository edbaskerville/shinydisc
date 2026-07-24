use std::path::PathBuf;

use serde::{Serialize, Deserialize};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    output_dir: PathBuf,
    update_all_metadata: bool,
    gps_coords: String,
}

impl Config {
    fn new(app: &AppHandle) -> Self {
        Self {
            output_dir: app.path().picture_dir().unwrap().join("shinydisc"),
            update_all_metadata: false,
            gps_coords: "37.78401, -122.50331".into()
        }
    }
}

fn config_dir(app: &AppHandle) -> PathBuf {
    app.path().app_config_dir().unwrap()
}

fn config_path(app: &AppHandle) -> PathBuf {
    config_dir(app).join("config.json")
}

pub fn get_config(app: &AppHandle) -> Config {
    // TODO: fix control flow/duplication
    let config_path = config_path(app);
    if config_path.exists() {
        let file = std::fs::File::open(&config_path).unwrap();
        match serde_json::from_reader(file) {
            Ok(config) => config,
            Err(e) => {
                // Reset config if we fail to read it.
                // Most likely scenario: using old version.
                let config = Config::new(app);
                write_config(app, &config);
                config
            }
        }
    }
    else {
        let config = Config::new(app);
        write_config(app, &config);
        config
    }
}

fn write_config(app: &AppHandle, config: &Config) {
    let config_path = config_path(app);
    println!("config_path: {:?}", config_path);
    let file = std::fs::File::options()
    .create(true)
    .truncate(true)
    .write(true)
    .open(&config_path).unwrap();
    serde_json::to_writer(file, config).unwrap();
}

pub fn set_output_dir(app: &AppHandle, output_dir: PathBuf) {
    let mut config = get_config(app);
    config.output_dir = output_dir;
    write_config(app, &config);
}

pub fn set_update_all_metadata(app: &AppHandle, update_all_metdata: bool) {
    let mut config = get_config(app);
    config.update_all_metadata = update_all_metdata;
    write_config(app, &config);
}

pub fn set_gps_coords(app: &AppHandle, gps_coords: String) {
    let mut config = get_config(app);
    config.gps_coords = gps_coords;
    write_config(app, &config);
}

pub fn should_update_all_metadata(app: &AppHandle) -> bool {
    get_config(app).update_all_metadata
}

pub fn get_gps_coords(app: &AppHandle) -> String {
    get_config(app).gps_coords
}

pub fn get_output_dir(app: &AppHandle) -> PathBuf {
    get_config(app).output_dir
}
