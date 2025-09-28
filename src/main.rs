mod config;
mod connection;
mod entity;
mod key;
mod queue;
mod sync_watcher;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::Mutex;
use tokio::sync::watch::channel;
use tokio::time::sleep;

use self::connection::Connection;
use self::entity::CommAction;
use self::sync_watcher::{SyncProcess, SyncWatcher};

#[tokio::main]
async fn main() -> Result<()> {
    // TODO: retrieve from cli, like --config "path"
    let user_relative_path = "";
    let config = config::Config::new(user_relative_path).unwrap();

    // setup the connection
    println!("starting connection");
    let tmp_dir = std::env::temp_dir().join("fsy_storage");
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let conn = Arc::new(Mutex::new(
        Connection::new(&config.local.secret_key, &tmp_dir).await?,
    ));
    let node_id = conn.lock().await.get_node_id();
    println!("- waiting for requests. public id: {node_id}");

    // setup the process
    println!("initializing sync process");
    let sync_process = SyncProcess::new(config.file_syncs.clone(), config.trustees.clone());

    // setup the queues
    let raw_queue: queue::Queue<CommAction> = queue::Queue::new(queue::MAX_CAPACITY);
    let conn_queue: Arc<Mutex<queue::Queue<CommAction>>> = Arc::new(Mutex::new(raw_queue.clone()));
    let sync_queue: Arc<Mutex<queue::Queue<CommAction>>> = Arc::new(Mutex::new(raw_queue));

    // NOTE: controller if the app is running or not
    let (is_running_tx, is_running_rx) = channel(true);

    // loop receivers of events into queues
    let event_is_running_rx = is_running_rx.clone();
    let event_conn = conn.clone();
    let event_conn_queue = conn_queue.clone();
    let event_sync_process = sync_process.clone();
    let event_sync_queue = sync_queue.clone();
    tokio::spawn(async move {
        println!("starting watcher sync");
        let mut sync_watcher =
            SyncWatcher::new(event_sync_process, config.local.push_debounce_millisecs).unwrap();
        sync_watcher.start().unwrap();

        println!("looping event checker");
        loop {
            if !*event_is_running_rx.borrow() {
                break;
            }

            sync_watcher = run_event_check(
                &event_conn,
                sync_watcher,
                &event_conn_queue,
                &event_sync_queue,
            )
            .await
            .unwrap();
            sleep(Duration::from_millis(config.local.loop_debounce_millisecs)).await;
        }

        sync_watcher.close().unwrap();
    });

    // handle the queues
    let queue_is_running_rx = is_running_rx.clone();
    let queue_conn_queue = conn_queue.clone();
    let queue_conn = conn.clone();
    let queue_sync_queue = sync_queue.clone();
    tokio::spawn(async move {
        println!("looping queues");
        loop {
            if !*queue_is_running_rx.borrow() {
                break;
            }

            run_queue_check(
                &queue_conn,
                &sync_process,
                &queue_conn_queue,
                &queue_sync_queue,
            )
            .await
            .unwrap();
            sleep(Duration::from_millis(config.local.loop_debounce_millisecs)).await;
        }
    });

    // wait for all the keyboard events
    // included will be the signal exit
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for event");
    println!("closing");

    // shut the threads
    is_running_tx.send(false).unwrap();

    // NOTE: when it arrives here, it means we should close all
    conn.lock().await.close().await.unwrap();

    Ok(())
}

async fn run_event_check(
    conn: &Arc<Mutex<Connection>>,
    sync: SyncWatcher,
    conn_queue: &Arc<Mutex<queue::Queue<CommAction>>>,
    sync_queue: &Arc<Mutex<queue::Queue<CommAction>>>,
) -> Result<SyncWatcher> {
    // check for events on the connection
    {
        // NOTE: setup scope because of the lock
        if let Ok(Some(msg)) = conn.lock().await.get_events() {
            sync_queue.lock().await.push(msg);
        }
    }

    // check for events on the watcher
    if let Some(msgs) = sync.get_changed_files() {
        if !msgs.is_empty() {
            let mut queue = conn_queue.lock().await;
            for msg in msgs {
                queue.push(msg);
            }
        }
    }

    Ok(sync)
}

async fn run_queue_check(
    conn: &Arc<Mutex<Connection>>,
    _sync: &SyncProcess,
    conn_queue: &Arc<Mutex<queue::Queue<CommAction>>>,
    sync_queue: &Arc<Mutex<queue::Queue<CommAction>>>,
) -> Result<()> {
    let conn_action: Option<CommAction>;
    let sync_action: Option<CommAction>;
    {
        // NOTE: setup scope because of the lock, we need to remove the lock asap
        conn_action = conn_queue.lock().await.pop();
        sync_action = sync_queue.lock().await.pop();
    }

    // handle actions incoming to the connection
    if let Some(CommAction::SendMessage(node_id, msg)) = conn_action {
        println!("conn_queue: sending message");
        println!("- \"{msg}\" to node: \"{node_id}\"");
        if let Err(e) = conn.lock().await.send_msg_to_node(node_id, msg).await {
            println!("- error: {e}");
        }
    }

    if let Some(CommAction::ReceiveMessage(node_id, msg)) = sync_action {
        println!("sync_queue: receiving message");
        println!("- \"{msg}\" to node: \"{node_id}\"");
    }

    // TODO: check if key is fine
    // TODO: check if node has a pull
    // TODO: check if msg is needed and if we want to download, if we want, request the ticket
    // TODO: if the msg is a download message, provide the blob id
    // TODO: if the msg is a blob id, download

    Ok(())
}
