use crate::{connection, config::{FileSync, NodeData, TargetMode}};
use notify::FsEventWatcher;
use notify_debouncer_mini::{DebounceEventResult, DebouncedEventKind, Debouncer, new_debouncer};
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tokio::{select, sync::watch::{Sender, Receiver, channel}};

pub struct SyncWatcher<'a> {
    conn: &'a mut connection::Connection,
    push_watcher: Debouncer<FsEventWatcher>,
    push_watcher_rx: Receiver<Option<PathBuf>>,
    syncs: &'a [FileSync],
    nodes: &'a [NodeData],
}
impl<'a> SyncWatcher<'a> {
    pub fn new(
        // TODO: maybe just need send message, receive file and send file?!
        conn: &'a mut connection::Connection,
        syncs: &'a [FileSync],
        nodes: &'a [NodeData],
        push_debounce_secs: u64,
    ) -> Result<Self, Box<dyn Error>> {
        let (watcher_tx, watcher_rx) = channel::<Option<PathBuf>>(None);

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
        )?;

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
    pub async fn start(&mut self, is_running_rx: &mut Receiver<bool>) {
        // maybe it is not running already
        if !*is_running_rx.borrow() {
            return;
        }

        let (pull_watcher_tx, mut pull_watcher_rx) = channel::<Option<(String, String)>>(None);

        // setup all
        self.set_push();
        self.set_pull(pull_watcher_tx);

        // TODO: should force one check right at the start

        loop {
            select! {
                // if not running, get out
                _ = is_running_rx.changed() => {
                    if !*is_running_rx.borrow() {
                        break;
                    }
                },

                // check for evts for push
                _ = self.push_watcher_rx.changed() => {
                    let changed_path: Option<PathBuf> = self.push_watcher_rx.borrow().clone();
                    if let Some(changed_path) = changed_path {
                        self.send_push(changed_path).await;
                    }
                },

                // check for evts on the pull
                _ = pull_watcher_rx.changed() => {
                    let sub_msg: Option<(String, String)> = pull_watcher_rx.borrow().clone();
                    if let Some(sub_msg) = sub_msg {
                        self.receive_pull_msg(sub_msg.0, sub_msg.1);
                    }
                },
            }
        }
    }

    // close handles the unsetup of the whole watcher
    pub fn close(&mut self) -> Result<(), Box<dyn Error>> {
        // handle the push side
        for sync in self.syncs.iter() {
            let sync_path = sync.path.clone();
            let p = std::path::Path::new(&sync_path);

            // TODO: we just want to ignore error and unwatch all
            self.push_watcher.watcher().unwatch(p)?;
        }

        // handle the pull side
        self.conn.unsubscribe_from_msgs();

        // TODO: do we need to do anything?!
        Ok(())
    }

    fn set_push(&mut self) -> Result<(), Box<dyn Error>> {
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
            let meta = fs::metadata(&sync_path)?;
            let recurse = if meta.is_dir() {
                notify::RecursiveMode::Recursive
            } else {
                notify::RecursiveMode::NonRecursive
            };

            let p = std::path::Path::new(&sync_path);
            self.push_watcher.watcher().watch(p, recurse)?;
        }

        Ok(())
    }

    fn set_pull(&mut self, pull_watcher_tx: Sender<Option<(String, String)>>) {
        println!("setting pull");
        self.conn.subscribe_to_msgs(pull_watcher_tx);
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

        // iterate each sync and targets send through the node
        for sync in list {
            let has_push = sync.targets.iter().any(|t| t.mode == TargetMode::Push || t.mode == TargetMode::PushPull);
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
                let node = self.nodes.iter().find(|n| n.name == t.trustee_name);
                if let Some(node) = node {
                    // TODO: should probably setup on a different thread
                    let res = self.send_push_to_node(sync, node).await;
                    if let Err(e) = res {
                        println!("error on sending message to node: {e}");
                        // TODO: what to do with the error?!
                        // TODO: it can be a timeout and we should be fine
                    }
                }
            }
        }
    }

    async fn send_push_to_node(&self, sync: &FileSync, node: &NodeData) -> Result<(), Box<dyn Error>> {
        println!("=> SENDING PUSH: {:?} TO {:?}", &sync.path, &node.node_id);

        let msg = "key(PATH;DATE;CHECKSUM / TICKET ID?!)";

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

        // TODO: inform the node that he can download a new version
        self.conn.send_msg_to_node(&node.node_id, msg).await
    }

    fn receive_pull_msg(&self, node_id: String, msg: String) {
        println!("=> RECEIVING PULL: {:?} TO {:?}", &node_id, &msg);
        // TODO: check if key is fine
        // TODO: check if node has a pull
        // TODO: check if msg is needed and if we want to download, if we want, request the ticket
        // TODO: if the msg is a download message, provide the blob id
        // TODO: if the msg is a blob id, download
    }
}
