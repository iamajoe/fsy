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
use self::sync_watcher::SyncWatcher;

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
    println!("waiting for requests. public id: {node_id}");

    // setup the watcher
    println!("starting watcher sync");
    let sync = Arc::new(Mutex::new(SyncWatcher::new(
        config.file_syncs,
        config.trustees,
        config.local.push_debounce_millisecs,
    )?));

    let (is_running_tx, is_running_rx) = channel(true);
    let conn_queue: Arc<Mutex<queue::Queue<CommAction>>> =
        Arc::new(Mutex::new(queue::Queue::new(queue::MAX_CAPACITY)));
    let sync_queue: Arc<Mutex<queue::Queue<CommAction>>> =
        Arc::new(Mutex::new(queue::Queue::new(queue::MAX_CAPACITY)));

    // loop receivers of events into queues
    println!("thread: looping events for queue");
    let event_is_running_rx = is_running_rx.clone();
    let event_conn_queue = conn_queue.clone();
    let event_conn = conn.clone();
    let event_sync_queue = sync_queue.clone();
    let event_sync = sync.clone();
    tokio::spawn(async move {
        loop {
            if !*event_is_running_rx.borrow() {
                break;
            }

            run_event_check(
                &event_conn,
                &event_sync,
                &event_conn_queue,
                &event_sync_queue,
            )
            .await
            .unwrap();
            sleep(Duration::from_millis(config.local.loop_debounce_millisecs)).await;
        }
    });

    // handle the queues
    println!("thread: looping queue handling");
    let queue_is_running_rx = is_running_rx.clone();
    let queue_conn_queue = conn_queue.clone();
    let queue_conn = conn.clone();
    let queue_sync_queue = sync_queue.clone();
    let queue_sync = sync.clone();
    tokio::spawn(async move {
        loop {
            if !*queue_is_running_rx.borrow() {
                break;
            }

            run_queue_check(
                &queue_conn,
                &queue_sync,
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
    sync.lock().await.close().unwrap();

    Ok(())
}

async fn run_event_check(
    conn: &Arc<Mutex<Connection>>,
    sync: &Arc<Mutex<SyncWatcher>>,
    conn_queue: &Arc<Mutex<queue::Queue<CommAction>>>,
    sync_queue: &Arc<Mutex<queue::Queue<CommAction>>>,
) -> Result<()> {
    // check for events on the connection
    if let Some(msg) = conn.lock().await.get_events() {
        sync_queue.lock().await.push(msg);
    }

    // check for events on the watcher
    if let Some(msgs) = sync.lock().await.get_changed_files() {
        for msg in msgs {
            conn_queue.lock().await.push(msg);
        }
    }

    Ok(())
}

async fn run_queue_check(
    _conn: &Arc<Mutex<Connection>>,
    _sync: &Arc<Mutex<SyncWatcher>>,
    _conn_queue: &Arc<Mutex<queue::Queue<CommAction>>>,
    _sync_queue: &Arc<Mutex<queue::Queue<CommAction>>>,
) -> Result<()> {
    // TODO: ...
    // check for events on the connection
    // TODO: handle the receiving part
    // TODO: send connection to the queue
    // println!("=> RECEIVING PULL: {:?} TO {:?}", &node_id, &msg);
    // TODO: check if key is fine
    // TODO: check if node has a pull
    // TODO: check if msg is needed and if we want to download, if we want, request the ticket
    // TODO: if the msg is a download message, provide the blob id
    // TODO: if the msg is a blob id, download

    // check for events on the watcher
    // TODO: handle the receiving part
    // TODO: send connection to the queue
    // println!("=> RECEIVING PUSH: {:?} TO {:?}", &node_id, &msg);
    // TODO: check if key is fine
    // TODO: check if node has a pull
    // TODO: check if msg is needed and if we want to download, if we want, request the ticket
    // TODO: if the msg is a download message, provide the blob id
    // TODO: if the msg is a blob id, download

    Ok(())
}
