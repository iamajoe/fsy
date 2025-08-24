use crate::*;
use serde::{Deserialize, Serialize};
use std::{env, ffi::OsString, fs, path::Path};

const CONFIG_FILE_NAME: &str = "fsy/config.toml";

#[derive(Serialize, Deserialize, Debug)]
pub struct LocalNodeData {
    pub public_key: String,
    pub secret_key: [u8; 32],
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NodeData {
    pub name: String,
    pub node_id: String,
    pub key: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileSyncTarget {
    pub mode: String, // TODO: should be an enum: push, push-pull, pull
    pub trustee: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileSync {
    pub name: String,
    pub path: String,
    pub target: Vec<FileSyncTarget>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub local: LocalNodeData,
    pub trustees: Vec<NodeData>,
    pub file_syncs: Vec<FileSync>,
}

impl Default for Config {
    fn default() -> Config {
        let raw_secret_key = key::generate_node_secret_key();

        Self {
            local: LocalNodeData {
                public_key: raw_secret_key.public().to_string(),
                secret_key: raw_secret_key.secret().to_bytes(),
            },
            trustees: vec![],
            file_syncs: vec![],
        }
    }
}

impl Config {
    pub fn new(user_relative_path: &str) -> Result<Self> {
        // being empty we want to create our own config
        let mut user_path = user_relative_path;
        if user_path.is_empty() {
            user_path = ".config";
        }

        let config_path = get_config_path(user_path).unwrap();
        dbg!(&config_path);

        // create the file if not there
        if !fs::exists(&config_path).unwrap() {
            return Self::default().save(user_path); // we want to save the file
        }

        // read the file now
        let content = fs::read_to_string(config_path).unwrap();
        let parsed: Config = toml::from_str(&content).unwrap();
        Ok(parsed)
    }

    pub fn save(self, user_relative_path: &str) -> Result<Self> {
        let config_path = get_config_path(user_relative_path).unwrap();
        let dir_name = match std::path::Path::new(&config_path).parent() {
            Some(p) => p,
            // TODO: handle the error
            None => {
                return Err(Error::Unknown);
            }
        };

        // make sure all directories are created
        if let Err(_e) = std::fs::create_dir_all(dir_name) {
            return Err(Error::Unknown);
        }

        let config_content = match toml::to_string(&self) {
            Ok(c) => c,
            // TODO: handle error!
            Err(_e) => {
                return Err(Error::Unknown);
            }
        };

        // write the config now
        if let Err(_e) = std::fs::write(&config_path, config_content) {
            return Err(Error::Unknown);
        }

        Ok(self)
    }
}

fn get_config_path(user_relative_path: &str) -> Result<OsString> {
    match std::env::var_os("HOME") {
        // handle home case
        Some(p) => Ok(Path::new(&p)
            .join(user_relative_path)
            .join(CONFIG_FILE_NAME)
            .into_os_string()),

        // handle case where there isn't an home
        None => match env::current_exe() {
            Ok(p) => Ok(p
                .parent()
                .unwrap()
                .join(user_relative_path)
                .join(CONFIG_FILE_NAME)
                .into_os_string()),
            // TODO: handle the error
            Err(_err) => Err(Error::Unknown),
        },
    }
}
