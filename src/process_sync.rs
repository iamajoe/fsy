use crate::config::{FileSync, NodeData, TargetMode};
use crate::{Result, connection};
use notify::Watcher;
use std::fs;
use std::sync::Arc;
use tokio::sync::watch::{self, Sender};

pub async fn process_sync(
    conn: connection::Connection,
    syncs: &Vec<FileSync>,
    nodes: &[NodeData],
) -> Result<Vec<Sender<bool>>> {
    let mut is_running_senders: Vec<Sender<bool>> = vec![];

    let conn = Arc::new(conn);
    for sync in syncs {
        // handle pushes
        if let Some(push_watch) = set_sync_push(Arc::clone(&conn), sync, nodes) {
            is_running_senders.push(push_watch);
        }

        // handle pulls
        // TODO: what to do here?!
    }

    Ok(is_running_senders)
}

fn set_sync_push(
    conn: Arc<connection::Connection>,
    sync: &FileSync,
    nodes: &[NodeData],
) -> Option<Sender<bool>> {
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
        println!("no targets provided");
        return None;
    }

    // no point in watching what doesn't exist
    if !fs::exists(&sync.path).unwrap() {
        println!("no file to sync exists");
        return None;
    }

    // figure which recursive mode should be used
    let sync_path = sync.path.clone();
    let meta = fs::metadata(&sync_path).unwrap();
    let recurse = if meta.is_dir() {
        notify::RecursiveMode::Recursive
    } else {
        notify::RecursiveMode::NonRecursive
    };

    println!("setting up sync target for sync...");

    // setup a new spawn for each process
    let (is_running_tx, is_running_rx) = watch::channel(true);

    tokio::spawn(async move {
        let (should_sync_tx, should_sync_rx) = watch::channel(false);
        let cb_tx = should_sync_tx.clone(); // value is moved to cb and we need to reset

        let mut watcher = notify::recommended_watcher(move |res| {
            if let Err(e) = res {
                println!("watch error: {:?}", e);
                return;
            }

            let _ = cb_tx.send(true);
        })
        .unwrap();
        let p = std::path::Path::new(&sync_path);
        watcher.watch(p, recurse).unwrap();

        // wait for the end
        let conn = Arc::clone(&conn);
        loop {
            let is_running = *is_running_rx.borrow();
            if !is_running {
                println!("shutting watcher for file: {}", &p.to_str().unwrap());
                watcher.unwatch(p).unwrap();
               break;
            }

            let should_sync = *should_sync_rx.borrow();
            if !should_sync {
                continue;
            }

            // time to sync...
            let _ = should_sync_tx.send(false);
            send_sync_targets(&conn, &sync_targets);
        }
    });

    // inform of the sender to close down the watcher
    Some(is_running_tx)
}

fn send_sync_targets(_conn: &connection::Connection, sync_targets: &[NodeData]) {
    let targets = sync_targets.to_vec();
    tokio::spawn(async move {
        for _node in targets {
            // TODO: need to handle errors
            // TODO: should be able to cancel the send
            // conn.send_msg_to_node(&node.node_id, &sync_path)
            //     .await
            //     .unwrap();

            // TODO: actually send to the node
            println!("sending target push...");
        }
    });
}
