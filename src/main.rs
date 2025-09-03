mod config;
mod connection;
mod error;
mod key;
mod process_sync;
mod tui;

use error::*;
use tokio::sync::watch;

#[tokio::main]
async fn main() -> Result<()> {
    // REF: https://github.com/n0-computer/iroh-blobs/blob/main/examples/transfer.rs

    let user_relative_path = "";
    let config = config::Config::new(user_relative_path).unwrap();

    println!("setting storage tmp folder...");
    let tmp_dir = std::env::temp_dir().join("fsy_storage");
    std::fs::create_dir_all(&tmp_dir).unwrap();

    println!("opening node...");
    let mut conn = connection::Connection::new(&config.local.secret_key, &tmp_dir).await?;
    let node_id = conn.get_node_id();
    println!("waiting for requests. public id: {node_id}");

    // TODO: log when a file has been synced

    // listen to config changes so we can setup all the syncing processes
    let (cwatch_is_running_tx, cwatch_is_running_rx) = watch::channel(true);
    tokio::spawn(async move {
        let mut config = config::Config::new(user_relative_path).unwrap();

        // setup the initial watchers for files
        let mut senders = process_sync::process_sync(&config.file_syncs, &config.trustees);

        // set a watcher in case config changes
        config.watch(cwatch_is_running_rx, |result| {
            match result {
                Ok(c) => {
                    // remove old senders
                    for sender in &senders {
                        // TODO: should probably log the error
                        let _ = sender.send(false);
                    }

                    println!("config changed, reloading");

                    // redo watchers for files
                    senders = process_sync::process_sync(&c.file_syncs, &c.trustees);
                }
                Err(e) => {
                    panic!("something went wrong with config watch {e}");
                }
            }
        });

        // watch was a blocker, as such, right now, we can close all senders
        for sender in &senders {
            // TODO: should probably log the error
            let _ = sender.send(false);
        }
    });

    // need the loop on a separate thread so we can listen for the ctrl_c signal
    let (tui_is_running_tx, tui_is_running_rx) = watch::channel(true);
    let tui_thread = tokio::spawn(async move {
        // TODO: how to pass the event signal?
        tui::init(&tui_is_running_rx, |evt| match evt {
            tui::Event::Quit => {
                tui_is_running_tx.send(false).unwrap();
                cwatch_is_running_tx.send(false).unwrap();
            }
        })
        .unwrap();
    });

    // wait for all the keyboard events
    // included will be the signal exit
    tui_thread.await.unwrap();

    println!("closing");
    conn.close().await?;

    Ok(())
}
