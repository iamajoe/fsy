use crate::{
    config::{FileSync, NodeData, TargetMode},
    entity::CommAction,
};
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
    syncs: Vec<FileSync>,
    nodes: Vec<NodeData>,
}
impl SyncWatcher {
    pub fn new(
        syncs: Vec<FileSync>,
        nodes: Vec<NodeData>,
        push_debounce_millisecs: u64,
    ) -> Result<Self> {
        let (watcher_tx, watcher_rx) = mpsc::channel();

        let mut watcher = new_debouncer(
            Duration::from_millis(push_debounce_millisecs),
            move |res: DebounceEventResult| match res {
                Ok(events) => events.iter().for_each(|e| {
                    if e.kind != DebouncedEventKind::Any {
                        return;
                    }

                    watcher_tx.send(Some(e.path.clone())).unwrap();
                }),
                Err(e) => println!("- watcher error {e}"),
            },
        )?;

        // set the pushes, pulls not needed, handled when come in
        let _ = set_push(&syncs, &mut watcher); // TODO: what about errors

        let s = Self {
            file_watcher: watcher,
            file_watcher_rx: watcher_rx,
            syncs,
            nodes,
        };

        Ok(s)
    }

    pub fn get_changed_files(&mut self) -> Option<Vec<CommAction>> {
        let changed_path = self.file_watcher_rx.try_recv();
        if let Ok(Some(changed_path)) = changed_path {
            let actions = get_push_comm_actions(&self.syncs, &self.nodes, changed_path);
            if !actions.is_empty() {
                return Some(actions);
            }
        }

        None
    }

    // close handles the unsetup of the whole watcher
    pub fn close(&mut self) -> Result<()> {
        // handle the push side
        for sync in self.syncs.iter() {
            let sync_path = sync.path.clone();
            let p = std::path::Path::new(&sync_path);

            // TODO: we just want to ignore error and unwatch all
            self.file_watcher.watcher().unwatch(p)?;
        }

        Ok(())
    }
}

fn set_push(syncs: &[FileSync], file_watcher: &mut Debouncer<RecommendedWatcher>) -> Result<()> {
    println!("setting push");

    // iterate each sync and set it up
    for sync in syncs.iter() {
        // handle pushes
        let is_push = sync
            .targets
            .iter()
            .any(|t| t.mode == TargetMode::Push || t.mode == TargetMode::PushPull);
        if !is_push {
            continue;
        }

        // only set watch if this is push
        let sync_path = sync.path.clone();
        println!("watching push file: {}", &sync_path);
        let meta = fs::metadata(&sync_path)?;
        let recurse = if meta.is_dir() {
            notify::RecursiveMode::Recursive
        } else {
            notify::RecursiveMode::NonRecursive
        };

        let p = std::path::Path::new(&sync_path);
        file_watcher.watcher().watch(p, recurse)?;
    }

    Ok(())
}

fn get_push_comm_actions(
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
                println!("=> SENDING PUSH: {:?}", &sync.path);
                actions.push(CommAction::SendMessage(
                    node.node_id.clone(),
                    get_push_sync_msg(sync),
                ));
            }
        }
    }

    actions
}

fn get_push_sync_msg(_sync: &FileSync) -> String {
    let msg = "key(PATH;DATE;CHECKSUM / TICKET ID?!)";
    msg.to_string()

    // TODO: what about the key?! we need to make sure the user can actually send
    //       we use the key to decrypt that depending on the node
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
