mod config;
mod connection;
mod entity;
mod queue;
mod key;
mod sync_watcher;

use anyhow::Result;
use tokio::select;
use tokio::sync::watch::{Receiver, Sender, channel};

use self::config::{FileSync, NodeData};
use self::connection::Connection;
use self::entity::CommAction;
use self::sync_watcher::SyncWatcher;

#[tokio::main]
async fn main() -> Result<()> {
    // TODO: retrieve from cli, like --config "path"
    let user_relative_path = "";
    let config = config::Config::new(user_relative_path).unwrap();

    let (is_running_tx, is_running_rx) = channel(true);

    let (conn_tx, conn_rx) = channel::<Vec<CommAction>>(vec![]);

    let sync_is_running_rx = is_running_rx.clone();
    let sync_conn_tx = conn_tx.clone();
    let sync_conn_rx = conn_rx.clone();

    // setup connection and wait for changes
    tokio::spawn(async move {
        println!("opening connection");
        let conn = init_conn(&config.local.secret_key).await.unwrap();
        let node_id = conn.get_node_id();
        println!("waiting for requests. public id: {node_id}");

        let conn = listen_to_conn(conn, is_running_rx, conn_tx, conn_rx)
            .await
            .unwrap();

        // NOTE: when it arrives here, it means the connection is no longer in use
        // close the connection
        conn.close().await.unwrap();
    });

    // setup sync watcher
    tokio::spawn(async move {
        println!("opening sync");
        let watcher = init_syncs(
            config.trustees,
            config.file_syncs,
            config.local.push_debounce_secs,
        )
        .unwrap();

        let mut watcher = listen_to_syncs(watcher, sync_is_running_rx, sync_conn_tx, sync_conn_rx)
            .await
            .unwrap();

        // NOTE: when it arrives here, it means the connection is no longer in use
        // close the connection
        watcher.close().unwrap();
    });

    // wait for all the keyboard events
    // included will be the signal exit
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for event");
    println!("closing");

    // shut the threads
    is_running_tx.send(false).unwrap();

    Ok(())
}

async fn init_conn(raw_secret_key: &[u8; 32]) -> Result<Connection> {
    let tmp_dir = std::env::temp_dir().join("fsy_storage");
    std::fs::create_dir_all(&tmp_dir).unwrap();

    let conn = Connection::new(raw_secret_key, &tmp_dir).await?;
    Ok(conn)
}

async fn listen_to_conn(
    mut conn: Connection,
    mut is_running_rx: Receiver<bool>,
    conn_tx: Sender<Vec<CommAction>>,
    mut conn_rx: Receiver<Vec<CommAction>>,
) -> Result<Connection> {
    // maybe it is not running already
    if !*is_running_rx.borrow() {
        return Ok(conn);
    }

    // TODO: need to setup a queue of events! otherwise older ones might be lost
    //       channels, as you push, they change the older value if not caught
    //       since it is a sync process to listen to changes and send them
    //       it is quite possible and probable that it happens

    // iterate and check for possible events on the connection
    loop {
        select! {
            // if not running, get out
            _ = is_running_rx.changed() => {
                if !*is_running_rx.borrow() {
                    break;
                }
            },

            // check for receiving messages through the connection
            _ = conn.check_events(&conn_tx) => {},

            // check for messages that should be sent through the connection
            _ = conn_rx.changed() => {
                let actions: Vec<CommAction> = conn_rx.borrow().clone();
                for action in actions {
                    if let CommAction::SendMessage(node_id, msg) = action {
                        conn.send_msg_to_node(node_id, msg).await.unwrap();
                    }
                }
            },
        }
    }

    Ok(conn)
}

fn init_syncs(
    trustees: Vec<NodeData>,
    file_syncs: Vec<FileSync>,
    push_debounce_secs: u64,
) -> Result<SyncWatcher> {
    let watcher = SyncWatcher::new(file_syncs, trustees, push_debounce_secs)?;

    Ok(watcher)
}

async fn listen_to_syncs(
    mut watcher: SyncWatcher,
    mut is_running_rx: Receiver<bool>,
    conn_tx: Sender<Vec<CommAction>>,
    mut conn_rx: Receiver<Vec<CommAction>>,
) -> Result<SyncWatcher> {
    // maybe it is not running already
    if !*is_running_rx.borrow() {
        return Ok(watcher);
    }

    // TODO: need to setup a queue of events! otherwise older ones might be lost
    //       channels, as you push, they change the older value if not caught
    //       since it is a sync process to listen to changes and send them
    //       it is quite possible and probable that it happens

    // iterate and check for possible events on the connection
    loop {
        select! {
            // if not running, get out
            _ = is_running_rx.changed() => {
                if !*is_running_rx.borrow() {
                    break;
                }
            },

            // check for messages to send
            _ = watcher.check_for_changed_files(&conn_tx) => {},

            // check for messages from the connection
            _ = conn_rx.changed() => {
                let actions: Vec<CommAction> = conn_rx.borrow().clone();
                for action in actions {
                    if let CommAction::ReceiveMessage(node_id, msg) = action {
                        // TODO: ...
                        println!("=> RECEIVING PULL: {:?} TO {:?}", &node_id, &msg);
                        // TODO: check if key is fine
                        // TODO: check if node has a pull
                        // TODO: check if msg is needed and if we want to download, if we want, request the ticket
                        // TODO: if the msg is a download message, provide the blob id
                        // TODO: if the msg is a blob id, download
                    }
                }
            },
        }
    }

    Ok(watcher)
}
