use crate::{Error, Result, key};
use serde::{Deserialize, Serialize};
use std::{env, ffi::OsString, fs, path::Path};

const CONFIG_FILE_NAME: &str = "fsy/config.toml";

#[derive(Serialize, Deserialize, Debug)]
pub struct LocalNodeData {
    pub public_key: String,
    pub secret_key: [u8; 32],
    pub push_debounce_secs: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NodeData {
    pub name: String, // unique identifier of this node for the user
    pub node_id: String,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum TargetMode {
    #[serde(rename = "push")]
    Push,
    #[serde(rename = "push-pull")]
    PushPull,
    #[serde(rename = "pull")]
    Pull,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileSyncTarget {
    pub mode: TargetMode,     // is it only push? only pull? both?
    pub trustee_name: String, // trustee name, the descritive
    pub key: String,          // used to check if the user has access to target
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileSync {
    pub name: String, // name identifier to be passed as unique communicator between nodes
    pub path: String, // path for the file / folder
    pub targets: Vec<FileSyncTarget>, // targets to whom push / pull
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    #[serde(skip)]
    pub config_path: OsString,
    pub local: LocalNodeData,
    pub trustees: Vec<NodeData>,
    pub file_syncs: Vec<FileSync>,
}

impl Default for Config {
    fn default() -> Config {
        let raw_secret_key = key::generate_node_secret_key();

        Self {
            config_path: ".config".into(),
            local: LocalNodeData {
                public_key: raw_secret_key.public().to_string(),
                secret_key: raw_secret_key.secret().to_bytes(),
                push_debounce_secs: 10,
            },
            trustees: vec![],
            file_syncs: vec![],
        }
    }
}

impl Config {
    pub fn new(user_relative_path: &str) -> Result<Self> {
        let config_path = get_config_path(user_relative_path).unwrap();

        // create the file if not there
        if !fs::exists(&config_path).unwrap() {
            let s = Self {
                config_path,
                ..Default::default()
            };
            return s.save(); // we want to save the file
        }

        // read the file now
        let content = fs::read_to_string(&config_path).unwrap();
        let mut parsed: Config = toml::from_str(&content).unwrap();
        // update with the path since we are not serializing it into the file
        parsed.config_path = config_path;

        // NOTE: we regenerate then so we can use for testing for example
        //       only check if config exists because we are already generating
        //       when it is a new config file
        let should_generate_key = std::env::var("GENERATE_KEY")
            .unwrap_or("".to_string())
            .eq("true");
        if should_generate_key {
            // NOTE: we regenerate then so we can use for testing for example
            //       only check if config exists because we are already generating
            //       when it is a new config file
            let raw_secret_key = key::generate_node_secret_key();
            parsed.local.public_key = raw_secret_key.public().to_string();
            parsed.local.secret_key = raw_secret_key.secret().to_bytes();
        }

        Ok(parsed)
    }

    pub fn save(self) -> Result<Self> {
        let dir_name = match std::path::Path::new(&self.config_path).parent() {
            Some(p) => p,
            None => {
                return Err(Error::Str("unable to get parent".to_string()));
            }
        };

        // make sure all directories are created
        if let Err(_e) = std::fs::create_dir_all(dir_name) {
            return Err(Error::Str("unable to create all dirs".to_string()));
        }

        let config_content = match toml::to_string(&self) {
            Ok(c) => c,
            Err(_e) => {
                return Err(Error::Str(
                    "unable to change config to toml string".to_string(),
                ));
            }
        };

        // write the config now
        if let Err(_e) = std::fs::write(&self.config_path, config_content) {
            return Err(Error::Str("unable to write config file".to_string()));
        }

        Ok(self)
    }
}

fn get_config_path(user_relative_path: &str) -> Result<OsString> {
    // being empty we want to create our own config
    let mut user_path = user_relative_path;
    if user_path.is_empty() {
        user_path = ".config";
    }

    match std::env::var_os("HOME") {
        // handle home case
        Some(p) => Ok(Path::new(&p)
            .join(user_path)
            .join(CONFIG_FILE_NAME)
            .into_os_string()),

        // handle case where there isn't an home
        None => match env::current_exe() {
            Ok(p) => Ok(p
                .parent()
                .unwrap()
                .join(user_path)
                .join(CONFIG_FILE_NAME)
                .into_os_string()),
            Err(err) => Err(Error::Unknown(Box::new(err))),
        },
    }
}
