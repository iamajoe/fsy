use anyhow::Result;

use notify::RecommendedWatcher;
use notify_debouncer_mini::{DebounceEventResult, DebouncedEventKind, Debouncer, new_debouncer};

use std::path::PathBuf;
use std::time::Duration;
use std::{
    fs,
    sync::mpsc::{self, Receiver},
};

#[derive(Clone)]
pub struct ChangedTarget {
    pub base_path: String,
    pub relative_path: String,
}

pub struct PathWatcher {
    file_watcher: Debouncer<RecommendedWatcher>,
    file_watcher_rx: Receiver<Option<PathBuf>>,
    watch_paths: Vec<String>,
}

impl PathWatcher {
    pub fn new(push_paths: Vec<String>, push_debounce_millisecs: u64) -> Result<Self> {
        let (watcher_tx, watcher_rx) = mpsc::channel();

        // initialize the watcher
        let watcher = new_debouncer(
            Duration::from_millis(push_debounce_millisecs),
            move |res: DebounceEventResult| match res {
                Ok(events) => events.iter().for_each(|e| {
                    if e.kind != DebouncedEventKind::Any {
                        return;
                    }

                    watcher_tx.send(Some(e.path.clone())).unwrap();
                }),
                Err(e) => println!("-> watcher error {e}"),
            },
        )?;

        // construct the final struct
        let s = Self {
            watch_paths: push_paths,
            file_watcher: watcher,
            file_watcher_rx: watcher_rx,
        };

        Ok(s)
    }

    pub fn start(&mut self) -> Result<()> {
        // listen to file changes
        self.set_watcher_files()
    }

    pub fn get_changed_targets(&self) -> Option<Vec<ChangedTarget>> {
        let changed_path = self.file_watcher_rx.try_recv();
        if let Ok(Some(changed_path)) = changed_path {
            let targets = get_push_targets_with_file(&self.watch_paths, changed_path.to_str()?);
            if targets.is_empty() {
                return None;
            }

            return Some(targets);
        }

        None
    }

    // close handles the unsetup of the whole watcher
    pub fn close(&mut self) -> Result<()> {
        for sync_path in self.watch_paths.iter() {
            let p = std::path::Path::new(&sync_path);
            // TODO: we just want to ignore error and unwatch all
            self.file_watcher.watcher().unwatch(p)?;
        }

        Ok(())
    }

    fn set_watcher_files(&mut self) -> Result<()> {
        for sync_path in self.watch_paths.iter() {
            // set the watch on path
            let meta = fs::metadata(sync_path)?;
            let recurse = if meta.is_dir() {
                notify::RecursiveMode::Recursive
            } else {
                notify::RecursiveMode::NonRecursive
            };

            let p = std::path::Path::new(&sync_path);
            self.file_watcher.watcher().watch(p, recurse)?;
        }

        Ok(())
    }
}

fn get_push_targets_with_file(push_paths: &[String], file_path: &str) -> Vec<ChangedTarget> {
    push_paths.iter().filter_map(|base_path| {
        if !file_path.contains(base_path) {
            return None;
        }

        // this means the file is the same
        // TODO: need to actually test this
        if base_path == file_path {
            return Some(ChangedTarget{
                base_path: base_path.to_owned(),
                relative_path: "".to_owned(),
            });
        }

        // being a directory, we know we have a relative path
        let relative_path = file_path.replace(base_path, "");
        Some(ChangedTarget{
            base_path: base_path.to_owned(),
            relative_path,
        })
    })
    .collect()
}
