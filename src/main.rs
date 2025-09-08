mod config;
mod connection;
mod error;
mod key;
mod process_sync;
// mod tui;

use std::sync::Arc;

use error::*;

#[tokio::main]
async fn main() -> Result<()> {
    // REF: https://github.com/n0-computer/iroh-blobs/blob/main/examples/transfer.rs

    let user_relative_path = "";
    let config = config::Config::new(user_relative_path).unwrap();

    println!("setting storage tmp folder...");
    let tmp_dir = std::env::temp_dir().join("fsy_storage");
    std::fs::create_dir_all(&tmp_dir).unwrap();

    println!("opening node...");
    let conn = connection::Connection::new(&config.local.secret_key, &tmp_dir).await?;
    let node_id = conn.get_node_id();
    println!("waiting for requests. public id: {node_id}");

    let conn = Arc::new(conn);

    // listen to config changes so we can setup all the syncing processes
    let (cwatch_is_running_tx, cwatch_is_running_rx) = tokio::sync::watch::channel(true);
    let watcher_conn = Arc::clone(&conn);
    tokio::spawn(async move {
        watch_config(watcher_conn, user_relative_path, cwatch_is_running_rx)
            .await
            .unwrap();
    });

    // need the loop on a separate thread so we can listen for the ctrl_c signal
    // let (tui_is_running_tx, tui_is_running_rx) = watch::channel(true);
    // let tui_thread = tokio::spawn(async move {
    //     // TODO: how to pass the event signal?
    //     tui::init(&tui_is_running_rx, |evt| match evt {
    //         tui::Event::Quit => {
    //             println!("quit event. sending shutdowns");
    //             tui_is_running_tx.send(false).unwrap();
    //         }
    //     })
    //     .unwrap();
    // });

    // wait for all the keyboard events
    // included will be the signal exit
    // tui_thread.await.unwrap();
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for event");
    println!("closing");

    cwatch_is_running_tx.send(false).unwrap();

    // bring a connection from arc if there is only one reference
    let conn = Arc::into_inner(conn);
    match conn {
        Some(mut conn) => conn.close().await?,
        // TODO: this should error
        None => println!("unable to close the connection"),
    }

    Ok(())
}

async fn watch_config(
    conn: Arc<connection::Connection>,
    user_relative_path: &'static str,
    is_running: tokio::sync::watch::Receiver<bool>,
) -> Result<()> {
    let config = config::Config::new(user_relative_path).unwrap();
    let senders = process_sync::process_sync(conn, &config.file_syncs, &config.trustees)
        .await
        .unwrap();

    // TODO: setup a watcher for the config itself

    // loop so we can keep checking when we should cancel this
    loop {
        // TODO: i dont think this channel is working
        // check if still running or if already all canceled
        let is_running = *is_running.borrow();
        if is_running {
            continue;
        }

        break;
    }

    // TODO: log when a file has been synced

    println!("shutting down watcher senders");

    // watch was a blocker, as such, right now, we can close all senders
    for sender in &senders {
        // TODO: should probably log the error
        let _ = sender.send(false);
    }

    Ok(())
}
