use crate::{Error, Result, key};
use notify::Watcher;
use serde::{Deserialize, Serialize};
use std::{env, ffi::OsString, fs, path::Path, sync::mpsc};
use tokio::sync::watch::Receiver;

const CONFIG_FILE_NAME: &str = "fsy/config.toml";

#[derive(Serialize, Deserialize, Debug)]
pub struct LocalNodeData {
    pub public_key: String,
    pub secret_key: [u8; 32],
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NodeData {
    pub name: String, // just something descritive for the user
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
    pub mode: TargetMode,
    pub trustee_name: String, // trustee name, the descritive
    pub key: String,          // used to check if the user has access to target
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileSync {
    pub path: String, // path for the file / folder
    pub targets: Vec<FileSyncTarget>,
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

    fn reload(&mut self) {
        let content = fs::read_to_string(&self.config_path).unwrap();
        let parsed: Config = toml::from_str(&content).unwrap();

        self.file_syncs = parsed.file_syncs;
        self.trustees = parsed.trustees;
        // TODO: do we want to reload anything else?!
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

    pub fn watch(&mut self, is_running_rx: Receiver<bool>, mut cb: impl FnMut(Result<&Self>)) {
        let config_path_raw = self.config_path.clone();
        let config_path = Path::new(&config_path_raw);

        // setup the watcher
        let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let mut watcher = notify::recommended_watcher(tx).unwrap();
        watcher
            .watch(config_path, notify::RecursiveMode::NonRecursive)
            .unwrap();

        println!(
            "setting watcher for file: {}",
            &config_path.to_str().unwrap()
        );

        // block forever, printing out events as they come in
        for res in rx {
            // TODO: i dont think this channel is working
            // check if still running or if already all canceled
            let is_running = *is_running_rx.borrow();
            if !is_running {
                println!(
                    "shutting watcher for file: {}",
                    &config_path.to_str().unwrap()
                );
                watcher.unwatch(config_path).unwrap();
                break;
            }

            match res {
                Ok(_event) => {
                    self.reload();
                    cb(Ok(self));
                }
                Err(e) => {
                    println!("Something went wrong with watcher: {e}");
                    cb(Err(Error::Notify(e)));
                }
            }
        }
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
