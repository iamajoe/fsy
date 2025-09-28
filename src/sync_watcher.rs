use crate::{
    config::{FileSync, NodeData, TargetMode},
    entity::CommAction,
};
use anyhow::Result;

use chrono::Utc;
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

    pub fn get_changed_files(&self) -> Option<Vec<CommAction>> {
        let changed_path = self.file_watcher_rx.try_recv();
        if let Ok(Some(changed_path)) = changed_path {
            return self.process.get_changed_path_actions(changed_path);
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
    nodes: Vec<NodeData>,
}

impl SyncProcess {
    pub fn new(syncs: Vec<FileSync>, nodes: Vec<NodeData>) -> Self {
        Self { syncs, nodes }
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

    pub fn get_changed_path_actions(&self, changed_path: PathBuf) -> Option<Vec<CommAction>> {
        let actions = get_changed_sync_actions(&self.syncs, &self.nodes, changed_path);
        if !actions.is_empty() {
            return Some(actions);
        }

        None
    }
}

fn get_changed_sync_actions(
    syncs: &[FileSync],
    nodes: &[NodeData],
    changed_path: PathBuf,
) -> Vec<CommAction> {
    let mut actions: Vec<CommAction> = vec![];

    let list: Vec<&FileSync> = syncs
        .iter()
        .filter(|sync| {
            let sync_path = PathBuf::from(&sync.path);
            if sync_path != changed_path {
                return false;
            }

            let is_push = sync
                .targets
                .iter()
                .any(|t| t.mode == TargetMode::Push || t.mode == TargetMode::PushPull);
            if !is_push {
                return false;
            }

            true
        })
        .collect();

    // iterate each sync and targets send through the node
    for sync in list {
        let has_push = sync
            .targets
            .iter()
            .any(|t| t.mode == TargetMode::Push || t.mode == TargetMode::PushPull);
        if !has_push {
            continue;
        }

        // TODO: build the ticket id / checksum
        //       these take space on memory / disk, we need to make sure there is at least
        //       one target that is interested, we need to ping him and wait for his response

        for t in &sync.targets {
            let is_push = t.mode == TargetMode::Push || t.mode == TargetMode::PushPull;
            if !is_push {
                continue;
            }

            // TODO: do not send in if coming in after pull for example
            //       need to check chunk or something to make sure we don't loop
            //       ourselves when in push-pull modes

            // find the right trustee data
            let node = nodes.iter().find(|n| n.name == t.trustee_name);
            if let Some(node) = node {
                actions.push(CommAction::SendMessage(
                    node.node_id.clone(),
                    get_push_sync_msg(sync, &t.key),
                ));
            }
        }
    }

    actions
}

fn get_push_sync_msg(sync: &FileSync, _key: &str) -> String {
    let blob_ticket_id = "1234";
    let now = Utc::now();

    // TODO: what about the key?! we use the key to decrypt that depending on 
    format!("{};{now};{blob_ticket_id}", &sync.path).to_string()

    //
    // TODO: use iroh blob hashing to create the ticket
    // TODO: we send to the node the ticket id
    // TODO: on the node side, we download the ticket
    // TODO: what if the node is not on?! we don't want to keep this stuff opening
    //       right?! because the tickets will consume either memory or file system
    //       and files can be huge
    //       as such, we want to ping the node to see if he is interested in
    //       downloading
}
