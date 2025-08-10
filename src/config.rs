use serde::Deserialize;
use std::{env, fs, io};

#[path = "./key.rs"]
mod key;

const CONFIG_TMPL: &str = include_str!("./static/config_tmpl.toml");

#[derive(Deserialize, Debug)]
pub struct TrusteeData {
    pub name: String,
    pub host: String,
    pub key: String,
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub trustees: Vec<TrusteeData>,
}

pub fn fetch_config() -> io::Result<Config> {
    let current_dir = env::current_exe().expect("unable to get current directory");
    let current_dir = current_dir
        .parent()
        .expect("unable to get current directory parent");
    let config_file_path = current_dir.join(".fsy_config.toml");

    // file doesn't exist so create it
    if !fs::exists(&config_file_path)? {
        let result = fs::write(
            &config_file_path,
            CONFIG_TMPL.replace("{{key}}", key::get_random_key(20).as_str()),
        );
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
