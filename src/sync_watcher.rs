use crate::config::{FileSync, NodeData, TargetMode};
use crate::{Result, connection};
use notify::FsEventWatcher;
use notify_debouncer_mini::{DebounceEventResult, DebouncedEventKind, Debouncer, new_debouncer};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tokio::select;

pub struct SyncWatcher<'a> {
    conn: &'a connection::Connection,
    push_watcher: Debouncer<FsEventWatcher>,
    push_watcher_rx: tokio::sync::watch::Receiver<Option<PathBuf>>,
    syncs: &'a [FileSync],
    nodes: &'a [NodeData],
}
impl<'a> SyncWatcher<'a> {
    pub fn new(
        // TODO: maybe just need send message, receive file and send file?!
        conn: &'a connection::Connection,
        syncs: &'a [FileSync],
        nodes: &'a [NodeData],
        push_debounce_secs: u64,
    ) -> Result<Self> {
        let (watcher_tx, watcher_rx) = tokio::sync::watch::channel::<Option<PathBuf>>(None);
        let watcher = new_debouncer(
            Duration::from_secs(push_debounce_secs),
            move |res: DebounceEventResult| match res {
                Ok(events) => events.iter().for_each(|e| {
                    if e.kind != DebouncedEventKind::Any {
                        return;
                    }

                    println!("change on path: {:?}", &e.path);
                    let _ = watcher_tx.send(Some(e.path.clone()));
                }),
                Err(e) => println!("watcher error {e}"),
            },
        )
        .unwrap();

        let s = Self {
            conn,
            push_watcher: watcher,
            push_watcher_rx: watcher_rx,
            syncs,
            nodes,
        };

        Ok(s)
    }

    // iterates through the watcher
    pub async fn start(&mut self, mut is_running_rx: tokio::sync::watch::Receiver<bool>) {
        // maybe it is not running already
        if !*is_running_rx.borrow() {
            return;
        }

        // setup all
        self.set_push();
        self.set_pull();

        loop {
            select! {
                // if not running, get out
                _ = is_running_rx.changed() => {
                    if !*is_running_rx.borrow() {
                        break;
                    }
                },

                // otherwise check for evts for push
                _ = self.push_watcher_rx.changed() => {
                    let changed_path: Option<PathBuf> = self.push_watcher_rx.borrow().clone();
                    if let Some(changed_path) = changed_path {
                        self.send_push(changed_path).await;
                    }
                },
            }
        }
    }

    // close handles the unsetup of the whole watcher
    pub fn close(&mut self) -> Result<()> {
        for sync in self.syncs.iter() {
            let sync_path = sync.path.clone();
            let p = std::path::Path::new(&sync_path);

            // TODO: we just want to ignore error and unwatch all
            self.push_watcher.watcher().unwatch(p).unwrap();
        }

        // TODO: do we need to do anything?!
        Ok(())
    }

    fn set_push(&mut self) {
        println!("setting push");

        // iterate each sync and set it up
        for sync in self.syncs.iter() {
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
            let meta = fs::metadata(&sync_path).unwrap();
            let recurse = if meta.is_dir() {
                notify::RecursiveMode::Recursive
            } else {
                notify::RecursiveMode::NonRecursive
            };

            let p = std::path::Path::new(&sync_path);
            self.push_watcher.watcher().watch(p, recurse).unwrap();
        }
    }

    fn set_pull(&self) {
        println!("setting pull");

        // iterate each sync and set it up
        for sync in self.syncs.iter() {
            // handle pulls
            let is_pull = sync
                .targets
                .iter()
                .any(|t| t.mode == TargetMode::Pull || t.mode == TargetMode::PushPull);
            if !is_pull {
                continue;
            }

            // TODO: what to do here?!
        }
    }

    async fn send_push(&self, changed_path: PathBuf) {
        let list: Vec<&FileSync> = self
            .syncs
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

        // TODO: should probably setup on a different thread

        // iterate each sync and targets send through the node
        for sync in list {
            for t in &sync.targets {
                let is_push = t.mode == TargetMode::Push || t.mode == TargetMode::PushPull;
                if !is_push {
                    continue;
                }

                // find the right trustee data
                let node = self.nodes.iter().find(|n| n.name == t.trustee_name);
                if let Some(node) = node {
                    println!("=> SENDING PUSH: {:?} TO {:?}", &changed_path, &node.node_id);

                    // send it through the wire
                    // self.conn
                    //     .send_msg_to_node(&node.node_id, &sync.path)
                    //     .await
                    //     .unwrap();
                }
            }
        }
    }
}
