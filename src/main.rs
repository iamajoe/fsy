mod action;
mod config;
mod connection;
mod event_process;
mod key;
mod path_watcher;
mod queue;
mod target;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use tokio::sync::{Mutex, watch::channel};
use tokio::time::sleep;

use self::action::CommAction;
use self::connection::Connection;
use self::path_watcher::PathWatcher;

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
    let event_nodes = config.nodes.clone();
    let event_target_groups = config.target_groups.clone();
    tokio::spawn(async move {
        println!("starting watcher sync");
        let push_groups = target::get_push_group_paths(&event_target_groups);
        let push_debounce = config.local.push_debounce_millisecs;
        let mut path_watcher = PathWatcher::new(push_groups, push_debounce).unwrap();
        path_watcher.start().unwrap();

        println!("looping event checker");
        loop {
            if !*event_is_running_rx.borrow() {
                break;
            }

            // check for events on the connection
            let conn_event: Option<connection::ConnEvent>;
            {
                // NOTE: setup scope because of the lock
                conn_event = event_conn.lock().await.get_events().unwrap();
            }

            let new_actions = event_process::run(
                &event_nodes,
                &event_target_groups,
                conn_event,
                path_watcher.get_changed_targets(),
            )
            .await
            .unwrap();

            {
                // NOTE: setup scope because of the lock
                event_queue.lock().await.push_multiple(new_actions);
            }

            sleep(Duration::from_millis(config.local.loop_debounce_millisecs)).await;
        }

        path_watcher.close().unwrap();
    });

    // handle the queues
    let queue_is_running_rx = is_running_rx.clone();
    let queue_queue = actions_queue.clone();
    let queue_conn = conn.clone();
    let queue_nodes = config.nodes.clone();
    let queue_target_groups = config.target_groups.clone();
    tokio::spawn(async move {
        println!("looping queues");
        loop {
            if !*queue_is_running_rx.borrow() {
                break;
            }

            let last_action: Option<action::CommAction>;
            {
                // NOTE: setup scope because of the lock, we need to remove the lock asap
                last_action = queue_queue.lock().await.pop();
            }

            // process the queue
            if let Some(action) = last_action {
                if let action::CommAction::Unknown = action {
                    break;
                }

                let start = Utc::now().timestamp_millis();
                println!("[queue_check][action] start...");
                match action::perform_action(
                    &queue_target_groups,
                    &queue_nodes,
                    action,
                    &queue_conn,
                )
                .await
                {
                    Ok(new_actions) => {
                        queue_queue.lock().await.push_multiple(new_actions);
                    }
                    Err(e) => {
                        // NOTE: we don't want to mess the process if an error comes in, keep doing it
                        println!("- error: {e}");
                    }
                }
                let time_spent = Utc::now().timestamp_millis() - start;
                println!("[queue_check][action] end ({time_spent}ms)");
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
