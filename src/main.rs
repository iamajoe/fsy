mod action;
mod config;
mod connection;
mod key;
mod queue;
mod target;
mod target_watcher;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use tokio::sync::{Mutex, watch::channel};
use tokio::time::sleep;

use self::action::{CommAction, perform_action};
use self::connection::Connection;
use self::target_watcher::{SyncProcess, TargetWatcher};

#[tokio::main]
async fn main() -> Result<()> {
    let config = config::Config::new("").unwrap();

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
    let sync_process = SyncProcess::new(config.target_groups.clone());

    // setup the queues
    let actions_queue: queue::Queue<CommAction> = queue::Queue::new(queue::MAX_CAPACITY);
    let actions_queue: Arc<Mutex<queue::Queue<CommAction>>> =
        Arc::new(Mutex::new(actions_queue.clone()));

    // NOTE: controller if the app is running or not
    let (is_running_tx, is_running_rx) = channel(true);

    // loop receivers of events into queues
    let event_is_running_rx = is_running_rx.clone();
    let event_queue = actions_queue.clone();
    let event_conn = conn.clone();
    let event_sync_process = sync_process.clone();
    tokio::spawn(async move {
        println!("starting watcher sync");
        let mut target_watcher =
            TargetWatcher::new(event_sync_process, config.local.push_debounce_millisecs).unwrap();
        target_watcher.start().unwrap();

        println!("looping event checker");
        loop {
            if !*event_is_running_rx.borrow() {
                break;
            }

            target_watcher = run_event_check(&event_conn, target_watcher, &event_queue)
                .await
                .unwrap();
            sleep(Duration::from_millis(config.local.loop_debounce_millisecs)).await;
        }

        target_watcher.close().unwrap();
    });

    // handle the queues
    let queue_is_running_rx = is_running_rx.clone();
    let queue_queue = actions_queue.clone();
    let queue_conn = conn.clone();
    tokio::spawn(async move {
        println!("looping queues");
        loop {
            if !*queue_is_running_rx.borrow() {
                break;
            }

            if let Err(e) = run_queue_check(&queue_conn, &sync_process, &queue_queue).await {
                // NOTE: we don't want to mess the process if an error comes in, keep doing it
                println!("- error: {e}");
            }

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

// run_event_check is run when there is an event on the connection
// or the sync process. For example:
// - a received message through the connection
//   - it parses then the message to be of the type of action
// - targets have changed on the syncing process
//   - it creates then actions to send through the connection
async fn run_event_check(
    conn: &Arc<Mutex<Connection>>,
    target_watcher: TargetWatcher,
    actions_queue: &Arc<Mutex<queue::Queue<CommAction>>>,
) -> Result<TargetWatcher> {
    // check for events on the connection
    let conn_event: Option<connection::ConnEvent>;
    {
        // NOTE: setup scope because of the lock
        conn_event = conn.lock().await.get_events().unwrap();
    }

    // check for events on the connection
    if let Some(connection::ConnEvent::ReceivedMessage(node_id, raw_msg)) = conn_event {
        println!("[event_check][conn] message received: {node_id}");
        let action = action::CommAction::from_namespaced_msg(&node_id, &raw_msg);
        actions_queue.lock().await.push(action);
    }

    // check if watcher has changed targets events
    if let Some(targets) = target_watcher.get_changed_targets() {
        println!("[event_check][watcher] targets changed: {}", targets.len());
        let config = config::Config::new("").unwrap();
        let target_names: Vec<String> = targets.iter().map(|t| t.name.clone()).collect();
        let node_ids: Vec<String> = targets
            .iter()
            .flat_map(|target| {
                target.get_node_ids(
                    &config.nodes,
                    &[target::TargetMode::Push, target::TargetMode::PushPull],
                )
            })
            .collect();

        let target_changed_actions: Vec<CommAction> = target_names
            .iter()
            .flat_map(|target_name| {
                node_ids.iter().map(|node_id| {
                    CommAction::TargetHasChanged(node_id.to_owned(), target_name.clone())
                        .to_send_message()
                })
            })
            .collect();
        if !target_changed_actions.is_empty() {
            actions_queue
                .lock()
                .await
                .push_multiple(target_changed_actions);
        }
    }

    Ok(target_watcher)
}

// run_queue_check runs all the queue items we have be it for
// the connection or the syncing process. for example:
// - if on the connection, it converts the action and sends a message
// - if on the sync, it consumes an action and performs
async fn run_queue_check(
    conn: &Arc<Mutex<Connection>>,
    sync_process: &SyncProcess,
    actions_queue: &Arc<Mutex<queue::Queue<CommAction>>>,
) -> Result<()> {
    let action: Option<CommAction>;
    {
        // NOTE: setup scope because of the lock, we need to remove the lock asap
        action = actions_queue.lock().await.pop();
    }

    match action {
        Some(action) => {
            if let CommAction::Unknown = action {
                return Ok(());
            }

            let start = Utc::now().timestamp_millis();
            println!("[queue_check][action] start...");
            let res = perform_action(conn, sync_process, actions_queue, action).await;
            let time_spent = Utc::now().timestamp_millis() - start;
            println!("[queue_check][action] end ({time_spent}ms)");

            res
        }
        _ => Ok(()),
    }
}
