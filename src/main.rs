mod config;
mod connection;
mod error;
mod key;
mod tui;
mod watcher;

use std::{
    path::Path,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use config::TargetMode;
use error::*;
use tokio::sync::watch;

#[tokio::main]
async fn main() -> Result<()> {
    // REF: https://github.com/n0-computer/iroh-blobs/blob/main/examples/transfer.rs

    let user_relative_path = "";
    // TODO: error could be better
    let config = config::Config::new(user_relative_path).unwrap();

    println!("setting storage tmp folder...");
    let tmp_dir = std::env::temp_dir().join("fsy_storage");
    std::fs::create_dir_all(&tmp_dir).unwrap();

    println!("opening node...");
    let mut conn = connection::Connection::new(&config.local.secret_key, &tmp_dir).await?;
    let node_id = conn.get_node_id();
    println!("waiting for requests. public id: {node_id}");

    // TODO: use notify to watch for config changes
    // TODO: after config change, setup watchers on the file syncs
    // TODO: log when a file has been synced

    let arc_config = Arc::new(Mutex::new(config));

    // need the loop on a separate thread so we can listen for the ctrl_c signal
    let (sync_is_running_tx, sync_is_running_rx) = watch::channel(true);
    let config = Arc::clone(&arc_config);
    tokio::spawn(async move {
        loop {
            // check if still running or if already all canceled
            let is_running = *sync_is_running_rx.borrow();
            if !is_running {
                break;
            }

            // go per sync to see if we have anything to pull
            // NOTE: setting up a guard here so that the arc mutex guard
            //       is dropped before moving to the next await
            {
                let m_config = config.lock().unwrap();
                for sync in &m_config.file_syncs {
                    // TODO: is there a change? to any of the files? if not, no need
                    //       to go through

                    for target in &sync.targets {
                        // we just want targets that have push so we can
                        // setup the ticket and inform when a change happens
                        if target.mode == TargetMode::Pull {
                            continue;
                        }

                        // TODO: check for diffs upon the peer
                        // TODO: ping from time to time the nodes i need to pull from
                        //       connect to them, see if there is something new
                        //       request blob ticket ids
                    }
                }
            }

            // wait 10 second for the next iteration
            tokio::time::sleep(Duration::from_millis(10000)).await;
        }
    });

    // separate thread to listen for watch events
    let (watch_is_running_tx, watch_is_running_rx) = watch::channel(true);
    let config = Arc::clone(&arc_config);
    tokio::spawn(async move {
        let mut m_config = config.lock().unwrap();
        let config_path_raw = m_config.config_path.clone();
        let config_path = Path::new(&config_path_raw);
        // TODO: debounce the watch
        // TODO: dont forget that we need to check updated dates, checksums...

        // listen to changes on config
        watcher::watch_path(
            Path::new(&config_path),
            |_evt| {
                println!("config changed, reloading {}", m_config.local.public_key);
                m_config.reload_filesyncs();
                m_config.reload_trustees();

                // TODO: redo watchers for files
            },
            &watch_is_running_rx,
        )
        .unwrap();
    });

    // need the loop on a separate thread so we can listen for the ctrl_c signal
    let (tui_is_running_tx, tui_is_running_rx) = watch::channel(true);
    let tui_thread = tokio::spawn(async move {
        // TODO: how to pass the event signal?
        tui::init(&tui_is_running_rx, |evt| match evt {
            tui::Event::Quit => {
                sync_is_running_tx.send(false).unwrap();
                tui_is_running_tx.send(false).unwrap();
                watch_is_running_tx.send(false).unwrap();
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
