use crate::{
    key,
    target::{NodeData, TargetGroup},
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{env, ffi::OsString, fs, path::Path};

const CONFIG_FILE_NAME: &str = "fsy/config.toml";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LocalNodeData {
    pub public_key: String,
    pub secret_key: [u8; 32],
    pub push_debounce_millisecs: u64,
    pub loop_debounce_millisecs: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    #[serde(skip)]
    pub config_path: OsString,
    pub local: LocalNodeData,
    pub nodes: Vec<NodeData>,
    pub target_groups: Vec<TargetGroup>,
}

impl Default for Config {
    fn default() -> Config {
        let raw_secret_key = key::generate_node_secret_key();

        Self {
            config_path: ".config".into(),
            local: LocalNodeData {
                public_key: raw_secret_key.public().to_string(),
                secret_key: raw_secret_key.secret().to_bytes(),
                push_debounce_millisecs: 500,
                loop_debounce_millisecs: 250,
            },
            nodes: vec![],
            target_groups: vec![],
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

            return save_config(s);
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

        // make sure the configuration is valid
        validate_config(&parsed)?;

        Ok(parsed)
    }
}

fn validate_config(conf: &Config) -> Result<()> {
    // node names need to be unique
    for node_a in &conf.nodes {
        for node_b in &conf.nodes {
            if node_a.id == node_b.id || node_a.name != node_b.name {
                continue;
            }

            bail!("node names need to be unique");
        }
    }

    // target names need to be unique
    for target_a in &conf.target_groups {
        for target_b in &conf.target_groups {
            if target_a.path == target_b.path || target_a.name != target_b.name {
                continue;
            }

            bail!("target group names need to be unique");
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
        None => {
            let p = env::current_exe()?;
            let res = p
                .parent()
                .unwrap()
                .join(user_path)
                .join(CONFIG_FILE_NAME)
                .into_os_string();

            Ok(res)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_config_path() -> Result<()> {
        let user_relative_path = "test_user_relative_path";
        let res = get_config_path(user_relative_path)?;
        let res_str = res.into_string().unwrap();

        assert!(&res_str.contains(user_relative_path));
        Ok(())
    }
}
