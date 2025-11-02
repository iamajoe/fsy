mod action;
mod config;
mod connection;
mod path_watcher;
mod target;

use std::time::Duration;

use anyhow::{Result, anyhow};
use chrono::Utc;
use tokio::sync::{mpsc, watch};
use tokio::time::sleep;

use self::path_watcher::PathWatcher;

const CHANNEL_BUFFER_SIZE: usize = 1000;

#[tokio::main]
async fn main() -> Result<()> {
    let config = config::Config::new("").unwrap();

    // NOTE: controller if the app is running or not
    let (is_running_tx, is_running_rx) = watch::channel(true);
    let (conn_events_tx, mut conn_events_rx) = mpsc::channel(CHANNEL_BUFFER_SIZE);
    let (actions_tx, mut actions_rx) = mpsc::channel(CHANNEL_BUFFER_SIZE);
    let (conn_event_result_tx, conn_event_result_rx) = watch::channel(None);

    // loop receivers of connection
    let conn_is_running_rx = is_running_rx.clone();
    let conn_actions_tx = actions_tx.clone();
    tokio::spawn(async move {
        println!("[conn] starting");

        let tmp_dir = std::env::temp_dir().join("fsy_storage");
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut conn = connection::Connection::new(&config.local.secret_key, &tmp_dir)
            .await
            .unwrap();
        let node_id = conn.get_node_id();
        println!("[conn] waiting for requests. public id: {node_id}");

        loop {
            if !*conn_is_running_rx.borrow() {
                break;
            }

            // handle the events incoming to the connection
            match conn.get_events() {
                Ok(evts) => {
                    for evt in evts {
                        println!("[conn][receive_event]");
                        if let connection::ConnEvent::ReceivedMessage(node_id, raw_msg) = evt {
                            println!("[conn][receive_event] message received: {node_id}");
                            let action =
                                action::CommAction::from_namespaced_msg(&node_id, &raw_msg);

                            // send the action to be handled by the process
                            if let Err(e) = conn_actions_tx.send(action).await {
                                // NOTE: we don't want to mess the process if an error comes in, keep doing it
                                println!("[conn][receive_event][action_send] error: {e}");
                            }
                        }
                    }
                }
                Err(e) => {
                    // NOTE: we don't want to mess the process if an error comes in, keep doing it
                    println!("[conn] error: {e}");
                }
            }

            // handle the events incoming from other threads
            while let Ok(event) = conn_events_rx.try_recv() {
                println!("[conn][event_send]");
                match connection::process_conn_event(event, &conn).await {
                    Ok(res) => {
                        if let Err(e) = conn_event_result_tx.send(res) {
                            // NOTE: we don't want to mess the process if an error comes in, keep doing it
                            println!("[conn][event_send][tx] error: {e}");
                        }
                    }
                    Err(e) => {
                        // NOTE: we don't want to mess the process if an error comes in, keep doing it
                        println!("[conn][event_send] error: {e}");
                    }
                }
            }

            sleep(Duration::from_millis(config.local.loop_debounce_millisecs)).await;
        }

        conn.close().await.unwrap();
    });

    // loop watcher
    let watcher_is_running_rx = is_running_rx.clone();
    let watcher_actions_tx = actions_tx.clone();
    let watcher_nodes = config.nodes.clone();
    let watcher_target_groups = config.target_groups.clone();
    tokio::spawn(async move {
        println!("[watcher] starting");
        let push_groups = target::get_push_group_paths(&watcher_target_groups);
        let push_debounce = config.local.push_debounce_millisecs;
        let mut path_watcher = PathWatcher::new(push_groups, push_debounce).unwrap();
        path_watcher.start().unwrap();

        println!("[watcher] looping");
        loop {
            if !*watcher_is_running_rx.borrow() {
                break;
            }

            // check for changed targets
            let new_actions = target::process_changed_targets(
                &watcher_nodes,
                &watcher_target_groups,
                path_watcher.get_changed_targets(),
            )
            .await
            .unwrap();
            for action in new_actions {
                println!("[watcher][action] sending action");

                // send the action to be handled by the process
                if let Err(e) = watcher_actions_tx.send(action).await {
                    // NOTE: we don't want to mess the process if an error comes in, keep doing it
                    println!("[watcher][action][action_send] error: {e}");
                }
            }

            sleep(Duration::from_millis(config.local.loop_debounce_millisecs)).await;
        }

        path_watcher.close().unwrap();
    });

    // handle the actions
    let actions_is_running_rx = is_running_rx.clone();
    let actions_actions_tx = actions_tx.clone();
    let actions_nodes = config.nodes.clone();
    let actions_target_groups = config.target_groups.clone();
    tokio::spawn(async move {
        println!("[actions] looping");
        loop {
            if !*actions_is_running_rx.borrow() {
                break;
            }

            // construct the closure function to retrieve the ticket id
            let fn_get_ticket_id = |file_path| {
                // NOTE: clone so we can send through the closure events
                let conn_events_tx = conn_events_tx.clone();
                let mut conn_event_result_rx = conn_event_result_rx.clone();
                async move {
                    // request the file ticket id
                    conn_events_tx
                        .send(connection::ConnEvent::GetFileTicket(file_path))
                        .await
                        .unwrap();

                    // wait for the ticket id to come
                    loop {
                        tokio::select! {
                            // wait for the ticket data to come in
                            _ = conn_event_result_rx.changed() => {
                                if let Some(connection::ConnEventResult::GetFileTicket(
                                    ticket_id,
                                )) = conn_event_result_rx.borrow().clone() {
                                    return Ok(ticket_id);
                                }
                            }

                            // timeout after 1 hour
                            _ = sleep(Duration::from_secs(3600)) => {
                                return Err(anyhow!("timeout waiting for ticket id"));
                            }
                        }
                    }
                }
            };

            // construct the closure function download ticket to path
            let fn_download_to_path = |ticket_id, file_path| {
                let conn_events_tx = conn_events_tx.clone();
                let mut conn_event_result_rx = conn_event_result_rx.clone();

                async move {
                    // request the download
                    conn_events_tx
                        .send(connection::ConnEvent::DownloadTicketToPath(
                            ticket_id, file_path,
                        ))
                        .await
                        .unwrap();

                    // wait for the download to come
                    loop {
                        tokio::select! {
                            // wait for the ticket data to come in
                            _ = conn_event_result_rx.changed() => {
                                if let Some(connection::ConnEventResult::DownloadTicketToPath) = conn_event_result_rx.borrow().clone() {
                                    return Ok(());
                                }
                            }

                            // timeout after 2 hour
                            _ = sleep(Duration::from_secs(7200)) => {
                                return Err(anyhow!("timeout waiting for download"));
                            }
                        }
                    }
                }
            };

            // handle the actions incoming from other threads
            while let Ok(action) = actions_rx.try_recv() {
                println!("[actions][receive]");

                let start = Utc::now().timestamp_millis();
                println!("[actions][perform] start...");
                match action::perform_action(
                    &actions_target_groups,
                    &actions_nodes,
                    action,
                    fn_get_ticket_id,
                    fn_download_to_path,
                )
                .await
                {
                    Ok((new_actions, new_conn_events)) => {
                        // send the actions to be handled by the process
                        for action in new_actions {
                            if let Err(e) = actions_actions_tx.send(action).await {
                                // NOTE: we don't want to mess the process if an error comes in, keep doing it
                                println!("[actions][perform][action_send] error: {e}");
                            }
                        }

                        // send the events to be handled by the process
                        for evt in new_conn_events {
                            if let Err(e) = conn_events_tx.send(evt).await {
                                // NOTE: we don't want to mess the process if an error comes in, keep doing it
                                println!("[actions][perform][conn_evt_send] error: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        // NOTE: we don't want to mess the process if an error comes in, keep doing it
                        println!("[actions][perform] error: {e}");
                    }
                }
                let time_spent = Utc::now().timestamp_millis() - start;
                println!("[actions][perform] end ({time_spent}ms)");
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

    Ok(())
}
