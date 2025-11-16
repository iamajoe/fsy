use anyhow::{Result, bail};
use iroh::SecretKey;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::{env, ffi::OsString, fs, path::Path};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum TargetMode {
    #[serde(rename = "push")]
    Push,
    #[serde(rename = "push-pull")]
    PushPull,
    #[serde(rename = "pull")]
    Pull,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum TargetKind {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "p2p")]
    P2p,
    #[serde(rename = "dropbox")]
    Dropbox,
    #[serde(rename = "s3")]
    S3,
}

impl fmt::Display for TargetKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TargetKind::Local => write!(f, "local"),
            TargetKind::P2p => write!(f, "p2p"),
            TargetKind::Dropbox => write!(f, "dropbox"),
            TargetKind::S3 => write!(f, "s3"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Target {
    pub enable: bool,
    pub id: String,
    pub mode: TargetMode,
    pub kind: TargetKind,
    pub src: String,
    pub ignore_nested_files: Option<Vec<String>>,

    // timing variables
    pub change_debounce_sec: Option<u64>,
    pub schedule_cron: Option<String>,

    // module data key
    pub data_key: Option<String>,     // s3, dropbox
    pub data_secret: Option<String>,  // s3
    pub data_node_id: Option<String>, // p2p
    pub data_src_id: Option<String>,  // p2p
    pub data_dest_id: Option<String>, // p2p
    pub data_dest: Option<String>,    // dropbox, local
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    #[serde(skip)]
    pub config_path: OsString,
    #[serde(skip)]
    pub db_path: OsString,
    #[serde(skip)]
    pub loop_sleep_time_ms: u64,

    // key related data
    pub p2p_public_key: String,
    pub p2p_secret_key: [u8; 32],

    // targets data
    pub targets: Vec<Target>,
}

impl Config {
    pub fn new(dir_path: &str) -> Result<Self> {
        let config_path = get_config_path(dir_path, "config.toml".to_owned())?;
        let db_path = get_config_path(dir_path, "db.db".to_owned())?;

        // create the file if not there
        if !fs::exists(&config_path).unwrap() {
            let raw_secret_key = generate_node_secret_key();
            let s = Self {
                config_path,
                db_path,
                loop_sleep_time_ms: 1000,
                p2p_public_key: raw_secret_key.public().to_string(),
                p2p_secret_key: raw_secret_key.secret().to_bytes(),
                targets: vec![],
            };

            return save_config(s);
        }

        // read the file now
        let content = fs::read_to_string(&config_path).unwrap();
        let mut parsed: Config = toml::from_str(&content).unwrap();

        // update with the path since we are not serializing it into the file
        parsed.config_path = config_path;
        parsed.db_path = db_path;
        parsed.loop_sleep_time_ms = 1000;

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
            let raw_secret_key = generate_node_secret_key();
            parsed.p2p_public_key = raw_secret_key.public().to_string();
            parsed.p2p_secret_key = raw_secret_key.secret().to_bytes();
        }

        // make sure the configuration is valid
        validate_config(&parsed)?;

        Ok(parsed)
    }
}

fn validate_config(conf: &Config) -> Result<()> {
    // target ids need to be unique
    for (i, target_a) in conf.targets.iter().enumerate() {
        for (c, target_b) in conf.targets.iter().enumerate() {
            if i == c {
                continue;
            }

            if target_a.id != target_b.id {
                continue;
            }

            bail!("target ids need to be unique");
        }
    }

    Ok(())
}

fn save_config(conf: Config) -> Result<Config> {
    let dir_name = match std::path::Path::new(&conf.config_path).parent() {
        Some(p) => p,
        None => {
            bail!("unable to get parent")
        }
    };

    // make sure all directories are created
    if let Err(_e) = std::fs::create_dir_all(dir_name) {
        bail!("unable to create all dirs")
    }

    let config_content = match toml::to_string(&conf) {
        Ok(c) => c,
        Err(_e) => {
            bail!("unable to change config to toml string")
        }
    };

    // write the config now
    if let Err(_e) = std::fs::write(&conf.config_path, config_content) {
        bail!("unable to write config file")
    }

    Ok(conf)
}

fn get_config_path(user_relative_path: &str, file_name: String) -> Result<OsString> {
    // being empty we want to create our own config
    let mut user_path = user_relative_path;
    if user_path.is_empty() {
        user_path = ".config/fsy";
    }

    match std::env::var_os("HOME") {
        // handle home case
        Some(p) => Ok(Path::new(&p)
            .join(user_path)
            .join(file_name)
            .into_os_string()),

        // handle case where there isn't an home
        None => {
            let p = env::current_exe()?;
            let res = p
                .parent()
                .unwrap()
                .join(user_path)
                .join(file_name)
                .into_os_string();

            Ok(res)
        }
    }
}

fn generate_node_secret_key() -> SecretKey {
    SecretKey::generate(rand::rngs::OsRng)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_config_path() -> Result<()> {
        let user_relative_path = "test_user_relative_path";
        let res = get_config_path(user_relative_path, "config.toml".to_owned())?;
        let res_str = res.into_string().unwrap();

        assert!(&res_str.contains(user_relative_path));
        Ok(())
    }
}
