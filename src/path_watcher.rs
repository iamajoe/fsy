use anyhow::Result;

use chrono::{DateTime, Utc};
use notify::RecommendedWatcher;
use notify_debouncer_mini::{DebounceEventResult, DebouncedEventKind, Debouncer, new_debouncer};

use std::path::PathBuf;
use std::time::Duration;
use std::{
    fs,
    sync::mpsc::{self, Receiver},
};

pub struct ChangedTarget {
    pub relative_path: String,
    pub full_path: String,
    pub timestamp: DateTime<Utc>,
}

pub struct PathWatcher {
    file_watcher: Debouncer<RecommendedWatcher>,
    file_watcher_rx: Receiver<Option<PathBuf>>,
    watch_paths: Vec<String>,
}

impl PathWatcher {
    pub fn new(push_paths: Vec<String>, push_debounce_millisecs: u64) -> Result<Self> {
        let (watcher_tx, watcher_rx) = mpsc::channel();

        // TODO: check if source is valid, if is directory...

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

        // TODO: handle the globs to listen to

        // construct the final struct
        let mut s = Self {
            watch_paths: push_paths,
            file_watcher: watcher,
            file_watcher_rx: watcher_rx,
        };

        // watch files
        s.set_watcher_files()?;

        Ok(s)
    }

    pub fn get_changed_target(&self) -> Result<Option<ChangedTarget>> {
        let changed_path = self.file_watcher_rx.try_recv();
        if let Ok(Some(changed_path)) = changed_path {
            let changed_path = changed_path.to_str().unwrap();
            return get_changed_target_from_path(&self.watch_paths, changed_path);
        }

        Ok(None)
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

fn get_changed_target_from_path(
    push_paths: &[String],
    file_path: &str,
) -> Result<Option<ChangedTarget>> {
    // get the modified timestamp in UTC
    let metadata = fs::metadata(file_path)?;
    let modified_time: DateTime<Utc> = metadata.modified()?.into();

    let result = push_paths.iter().find_map(|base_path| {
        if !file_path.contains(base_path) {
            return None;
        }

        let mut changed = ChangedTarget {
            relative_path: "".to_owned(),
            full_path: file_path.to_owned(),
            timestamp: modified_time,
        };

        // TODO: with glob, this might need to change

        // this means the file is the same
        // TODO: need to actually test this
        if base_path == file_path {
            return Some(changed);
        }

        // being a directory, we know we have a relative path
        let relative_path = file_path.replace(base_path, "");
        changed.relative_path = relative_path;

        Some(changed)
    });

    Ok(result)
}
