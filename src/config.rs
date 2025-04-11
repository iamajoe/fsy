use std::{collections::HashMap, env, fs, io};

use serde::Deserialize;

const CONFIG_TMPL: &str = "
[hosts]
local=\"127.0.0.1\"

[[sync]]
host=\"local\"
file=\"(change_with_your_path)\"
";

#[derive(Deserialize, Debug)]
pub struct SyncData {
    pub host: String,
    pub file: String,
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub hosts: HashMap<String, String>,

    #[serde(rename = "sync")]
    pub sync_list: Vec<SyncData>,
}

pub fn fetch_config() -> io::Result<Config> {
    let current_dir = env::current_exe().expect("unable to get current directory");
    let current_dir = current_dir
        .parent()
        .expect("unable to get current directory parent");
    let config_file_path = current_dir.join(".fsy_config.toml");

    // file doesn't exist so create it
    if !fs::exists(&config_file_path)? {
        let result = fs::write(&config_file_path, CONFIG_TMPL);
        match result {
            Err(err) => {
                return Err(err);
            }
            _ => {
                // do nothing on catchall
            }
        }
    }

    // read the file now
    let content = fs::read_to_string(config_file_path).expect("unable to read the config file");
    let parsed: Config = toml::from_str(&content).unwrap();
    Ok(parsed)
}
