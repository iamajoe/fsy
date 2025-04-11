use std::{env, fs, io};

use serde::Deserialize;

const CONFIG_TMPL: &str = "
[[hosts]]
name=\"local\"
host=\"127.0.0.1\"
username=\"foo\"
password=\"bar\"
ssh_file=\"(change_with_your_path)\"

[[sync]]
host=\"local\"
src=\"(change_with_your_path)\"
dest=\"(change_with_your_path)\"
";

#[derive(Deserialize, Debug)]
pub struct HostData {
    pub name: String,
    pub host: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub ssh_file: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct SyncData {
    pub host: String,
    pub src: String,
    pub dest: String,
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub hosts: Vec<HostData>,

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
