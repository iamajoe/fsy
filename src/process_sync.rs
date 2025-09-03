use std::sync::mpsc;

use crate::config::{FileSync, NodeData, TargetMode};
use notify::{Event, Watcher};
use std::fs;
use tokio::sync::watch::{self, Sender};

fn send_target_push(sync_path: String, node: &NodeData) {
    // TODO: actually send to the node
}

fn set_sync_push(sync: &FileSync, nodes: &[NodeData]) -> Option<Sender<bool>> {
    let sync_targets: Vec<NodeData> = sync
        .targets
        .iter()
        .filter(|t| t.mode == TargetMode::Push || t.mode == TargetMode::PushPull)
        .filter_map(|t| {
            nodes
                .iter()
                .find(|n| n.name == t.trustee_name)
                .map(|t| NodeData {
                    name: t.name.clone(),
                    node_id: t.node_id.clone(),
                })
        })
        .collect();

    // no point in watching if no targets to send to
    if sync_targets.is_empty() {
        return None;
    }

    // no point in watching what doesn't exist
    if !fs::exists(&sync.path).unwrap() {
        return None;
    }

    // figure which recursive mode should be used
    let meta = fs::metadata(&sync.path).unwrap();
    let recurse = if meta.is_dir() {
        notify::RecursiveMode::Recursive
    } else {
        notify::RecursiveMode::NonRecursive
    };

    // setup a new spawn for each process
    let (is_running_tx, is_running_rx) = watch::channel(true);
    let sync_path = sync.path.clone();

    tokio::spawn(async move {
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let watcher_res = notify::recommended_watcher(tx);

        // TODO: how to inform about the errors?!
        match watcher_res {
            Ok(mut watcher) => {
                // setup the watcher
                let p = std::path::Path::new(&sync_path);
                if let Err(err) = watcher.watch(p, recurse) {
                    // TODO: should we do something with the returning error?! probably panic
                    println!("watcher.watch error: {:?}", &err);
                    return;
                }

                // TODO: how to break from outside?

                // this is a blocking section
                for res in rx {
                    // nothing else to process on this watcher
                    let is_running = *is_running_rx.borrow();
                    if !is_running {
                        watcher.unwatch(p).unwrap();
                        break;
                    }

                    match res {
                        Ok(event) => {
                            // TODO: should push the sync
                            println!("event: {:?}", event);

                            for node in &sync_targets {
                                // TODO: need to handle errors
                                // TODO: should be able to cancel the send
                                send_target_push(sync_path.clone(), node);
                            }
                        }
                        Err(e) => println!("watch error: {:?}", e),
                    }
                }
            }
            Err(err) => {
                // TODO: should we do something with the returning error?! probably panic
                println!("recommended_watcher error: {:?}", &err);
            }
        }
    });

    // inform of the sender to close down the watcher
    Some(is_running_tx)
}

pub fn process_sync(syncs: &Vec<FileSync>, nodes: &[NodeData]) -> Vec<Sender<bool>> {
    let mut is_running_senders: Vec<Sender<bool>> = vec![];

    for sync in syncs {
        // handle pushes
        let push_watch = set_sync_push(sync, nodes);
        if let Some(s) = push_watch {
            is_running_senders.push(s)
        };

        // handle pulls
        // TODO: what to do here?!
    }

    is_running_senders
}
