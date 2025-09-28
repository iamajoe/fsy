use crate::config::{FileSync, FileSyncTarget, TargetMode};
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
            let targets = self.process.get_push_syncs_by_path(changed_path.to_str()?);
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

    pub fn get_push_syncs_by_path(&self, file_path: &str) -> Vec<FileSync> {
        get_syncs_by_path(
            &self.syncs,
            file_path,
            vec![TargetMode::Push, TargetMode::PushPull],
        )
    }

    pub fn get_pull_syncs_by_name(
        &self,
        target_name: &str,
        _timestamp: DateTime<Utc>,
    ) -> Vec<FileSync> {
        // TODO: what about timestamp?
        //       we should be careful so we don't download something
        //       that has just been downloaded for example
        get_syncs_by_name(
            &self.syncs,
            target_name,
            vec![TargetMode::Pull, TargetMode::PushPull],
        )
    }
}

fn get_syncs_by_path(
    syncs: &[FileSync],
    file_path: &str,
    target_modes: Vec<TargetMode>,
) -> Vec<FileSync> {
    let syncs: Vec<FileSync> = syncs.iter().filter_map(|sync| {
        if sync.path != file_path {
            return None;
        }

        Some(sync.clone())
    }).collect();

    get_syncs_by_target(&syncs, target_modes)
}

fn get_syncs_by_name(
    syncs: &[FileSync],
    target_name: &str,
    target_modes: Vec<TargetMode>,
) -> Vec<FileSync> {
    let syncs: Vec<FileSync> = syncs.iter().filter_map(|sync| {
        if sync.name != target_name {
            return None;
        }

        Some(sync.clone())
    }).collect();

    get_syncs_by_target(&syncs, target_modes)
}

fn get_syncs_by_target(syncs: &[FileSync], target_modes: Vec<TargetMode>) -> Vec<FileSync> {
    let mut file_targets: Vec<FileSync> = vec![];

    // iterate each sync to find all the targets we need
    for sync in syncs {
        // gather pushing targets
        let targets: Vec<FileSyncTarget> = sync
            .targets
            .iter()
            .filter_map(|t| {
                if !target_modes.contains(&t.mode) {
                    return None;
                }

                Some(t.clone())
            })
            .collect();
        if targets.is_empty() {
            continue;
        }

        // push the file target file
        file_targets.push(FileSync {
            name: sync.name.clone(),
            path: sync.path.clone(),
            targets,
        });
    }

    file_targets
}
