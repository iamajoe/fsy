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

pub struct WatchTarget {
    pub id: String,
    pub srcs: Vec<String>,
}

pub struct ChangedTarget {
    pub id: String,
    pub src: String,
    pub timestamp: DateTime<Utc>,
}

pub struct PathWatcher {
    file_watcher: Debouncer<RecommendedWatcher>,
    file_watcher_rx: Receiver<Option<PathBuf>>,
    watch_target: WatchTarget,
}

impl PathWatcher {
    pub fn new(watch_target: WatchTarget, debounce_millisecs: u64) -> Result<Self> {
        let (watcher_tx, watcher_rx) = mpsc::channel();

        // initialize the watcher
        let watcher = new_debouncer(
            Duration::from_millis(debounce_millisecs),
            move |res: DebounceEventResult| match res {
                Ok(events) => events.iter().for_each(|e| {
                    if e.kind != DebouncedEventKind::Any {
                        return;
                    }

                    println!("[path_watcher][file_changed] {}", &e.path.to_str().unwrap());
                    watcher_tx.send(Some(e.path.clone())).unwrap();
                }),
                Err(e) => println!("-> watcher error {e}"),
            },
        )?;

        // construct the final struct
        let mut s = Self {
            watch_target,
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
            let src = changed_path.to_str();
            if let Some(src) = src
                && let Ok(metadata) = fs::metadata(src)
            {
                let modified_time: DateTime<Utc> = metadata.modified()?.into();

                return Ok(Some(ChangedTarget {
                    id: self.watch_target.id.clone(),
                    src: src.to_owned(),
                    timestamp: modified_time,
                }));
            }
        }

        Ok(None)
    }

    // close handles the unsetup of the whole watcher
    pub fn close(&mut self) -> Result<()> {
        for sync_path in self.watch_target.srcs.iter() {
            let p = std::path::Path::new(&sync_path);
            // TODO: we just want to ignore error and unwatch all
            self.file_watcher.watcher().unwatch(p)?;
        }

        Ok(())
    }

    fn set_watcher_files(&mut self) -> Result<()> {
        for sync_path in self.watch_target.srcs.iter() {
            // set the watch on path
            if let Ok(meta) = fs::metadata(sync_path) {
                let recurse = if meta.is_dir() {
                    notify::RecursiveMode::Recursive
                } else {
                    notify::RecursiveMode::NonRecursive
                };

                let p = std::path::Path::new(&sync_path);
                self.file_watcher.watcher().watch(p, recurse)?;
            }
        }

        Ok(())
    }
}
