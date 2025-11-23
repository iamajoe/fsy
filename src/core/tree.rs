use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{self, Path};
use std::{fmt, fs};

use anyhow::{Result, anyhow};
use bao_tree::blake3;
use glob::glob;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Tree {
    full_path: String,
    relative_path: String,
    file_name: String,
    is_file: bool,
    children: Option<Vec<Tree>>,
}

impl Tree {
    pub fn from_path(raw: &str, ignore_globs: &[String]) -> Option<Self> {
        let ignore_paths = get_src_glob_paths(raw, ignore_globs);

        let relative = "".to_owned();
        path_to_tree(raw, &relative, &ignore_paths)
    }

    pub fn to_paths(&self) -> Vec<String> {
        tree_to_path(self)
    }

    pub fn to_toml(&self) -> Result<String> {
        match toml::to_string(self) {
            Ok(str) => Ok(str),
            Err(e) => Err(anyhow!(e)),
        }
    }

    pub fn from_toml(raw: &str) -> Result<Self> {
        match toml::from_str(raw) {
            Ok(tree) => Ok(tree),
            Err(e) => Err(anyhow!(e)),
        }
    }
}

impl fmt::Display for Tree {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.to_toml() {
            Ok(str) => write!(f, "{}", str),
            Err(e) => write!(f, "<invalid toml: {}>", e),
        }
    }
}

fn get_src_glob_paths(base: &str, globs: &[String]) -> Vec<String> {
    let mut paths: Vec<String> = vec![];
    for g in globs {
        // join the base to the glob so interacts only with the base
        let base_path = path::Path::new(base);
        let relative_glob = if let Some(joined) = base_path.join(g).to_str() {
            joined.to_owned()
        } else {
            base.to_owned()
        };

        // find all of the files for said glob
        if let Ok(entries) = glob(&relative_glob) {
            for entry in entries {
                if let Ok(p) = entry
                    && let Some(p) = p.to_str()
                {
                    paths.push(p.to_owned());
                }
            }
        }
    }

    paths
}

fn path_to_tree(
    curr_path: &str,
    parent_relative_path: &str,
    ignore_paths: &[String],
) -> Option<Tree> {
    // the file is ignored? then move on
    let ignore_found = ignore_paths.iter().any(|f| f == curr_path);
    if ignore_found {
        return None;
    }

    // handle the tree
    if let Ok(meta) = fs::metadata(curr_path) {
        let file_name = Path::new(&curr_path)
            .file_name()
            .and_then(|os| os.to_str())
            .unwrap_or("")
            .to_owned();

        let parent_relative_base = path::Path::new(&parent_relative_path);
        let relative_path = if let Some(joined) = parent_relative_base.join(&file_name).to_str() {
            joined.to_owned()
        } else {
            file_name.clone()
        };

        let mut target = Tree {
            file_name,
            full_path: curr_path.to_owned(),
            relative_path,
            is_file: true,
            children: None,
        };

        // handle file
        if !meta.is_dir() {
            println!(
                "FINGERPRINT {} {}",
                &target.file_name,
                fingerprint_file(&target.full_path).unwrap()
            );

            return Some(target);
        }

        // handle the directory
        target.is_file = false;
        if let Ok(child_paths) = fs::read_dir(curr_path) {
            let children: Vec<Tree> = child_paths
                .into_iter()
                .filter_map(|p| {
                    if let Ok(p) = p
                        && let Some(p) = p.path().to_str()
                    {
                        return path_to_tree(p, &target.relative_path, ignore_paths);
                    }

                    None
                })
                .collect();

            if !children.is_empty() {
                target.children = Some(children);
            }
        }

        return Some(target);
    }

    None
}

fn tree_to_path(tree: &Tree) -> Vec<String> {
    let mut arr = vec![tree.full_path.clone()];

    // handle the directory
    if !tree.is_file
        && let Some(children) = &tree.children
    {
        for child in children {
            for child_path in tree_to_path(child) {
                arr.push(child_path);
            }
        }
    }

    arr
}

pub fn fingerprint_file(file: &str) -> Result<String> {
    match File::open(file) {
        Ok(file) => {
            let mut file = BufReader::new(file);
            let mut hasher = blake3::Hasher::new();
            let mut buf = [0u8; 65536];

            loop {
                let n = file.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }

                hasher.update(&buf[..n]);
            }

            let res = hasher.finalize();
            Ok(res.to_string())
        }
        Err(e) => Err(anyhow!(e)),
    }
}
