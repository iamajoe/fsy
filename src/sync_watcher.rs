use crate::config::{FileSync, FileSyncTarget, TargetMode};
use anyhow::Result;

use notify::RecommendedWatcher;
use notify_debouncer_mini::{DebounceEventResult, DebouncedEventKind, Debouncer, new_debouncer};

use std::path::PathBuf;
use std::time::Duration;
use std::{
    fs,
    sync::mpsc::{self, Receiver},
};

pub struct SyncWatcher {
    file_watcher: Debouncer<RecommendedWatcher>,
    file_watcher_rx: Receiver<Option<PathBuf>>,
    process: SyncProcess,
    watch_paths: Vec<String>,
}

impl SyncWatcher {
    pub fn new(process: SyncProcess, push_debounce_millisecs: u64) -> Result<Self> {
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

        let watch_paths = process.get_paths_to_watch();

        // construct the final struct
        let s = Self {
            process,
            watch_paths,
            file_watcher: watcher,
            file_watcher_rx: watcher_rx,
        };

        Ok(s)
    }

    pub fn start(&mut self) -> Result<()> {
        // listen to file changes
        self.set_watcher_files()
    }

    pub fn get_changed_files(&self) -> Option<Vec<FileSync>> {
        let changed_path = self.file_watcher_rx.try_recv();
        if let Ok(Some(changed_path)) = changed_path {
            let targets = self.process.get_path_targets(changed_path);
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

#[derive(Clone)]
pub struct SyncProcess {
    syncs: Vec<FileSync>,
}

impl SyncProcess {
    pub fn new(syncs: Vec<FileSync>) -> Self {
        Self { syncs }
    }

    pub fn get_paths_to_watch(&self) -> Vec<String> {
        // find which paths we should be watching
        let mut watch_paths: Vec<String> = vec![];
        for sync in self.syncs.iter() {
            let is_push = sync
                .targets
                .iter()
                .any(|t| t.mode == TargetMode::Push || t.mode == TargetMode::PushPull);
            if !is_push {
                continue;
            }

            let sync_path = sync.path.clone();
            watch_paths.push(sync_path);
        }

        watch_paths
    }

    pub fn get_path_targets(&self, changed_path: PathBuf) -> Vec<FileSync> {
        get_path_sync_targets(&self.syncs, changed_path.clone())
    }
}

fn get_path_sync_targets(syncs: &[FileSync], file_path: PathBuf) -> Vec<FileSync> {
    let mut file_targets: Vec<FileSync> = vec![];

    // iterate each sync to find all the targets we need
    for sync in syncs {
        // check if path has changed
        let sync_path = PathBuf::from(&sync.path);
        if sync_path != file_path {
            continue;
        }

        // gather pushing targets
        let push_targets: Vec<FileSyncTarget> = sync
            .targets
            .iter()
            .filter_map(|t| {
                if t.mode != TargetMode::Push && t.mode != TargetMode::PushPull {
                    return None;
                }

                Some(t.clone())
            })
            .collect();
        if push_targets.is_empty() {
            continue;
        }

        // push the file target file
        file_targets.push(FileSync {
            name: sync.name.clone(),
            path: sync.path.clone(),
            targets: push_targets,
        });
    }

    file_targets
}
