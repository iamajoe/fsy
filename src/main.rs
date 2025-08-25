mod config;
mod connection;
mod error;
mod key;

use std::time::Duration;

use crossterm::{
    event,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use error::*;
use tokio::{sync::watch, time::sleep};

#[tokio::main]
async fn main() -> Result<()> {
    // REF: https://github.com/n0-computer/iroh-blobs/blob/main/examples/transfer.rs

    let user_relative_path = "";
    // TODO: error could be better
    let mut config = config::Config::new(user_relative_path).unwrap();

    println!("setting storage tmp folder...");
    let tmp_dir = std::env::temp_dir().join("fsy_storage");
    std::fs::create_dir_all(&tmp_dir).unwrap();

    println!("opening node...");
    let mut conn = connection::Connection::new(&config.local.secret_key, &tmp_dir).await?;
    let node_id = conn.get_node_id();
    println!("waiting for requests. public id: {node_id}");

    // need the loop on a separate thread so we can listen for the ctrl_c signal
    let (sync_is_running_tx, sync_is_running_rx) = watch::channel(true);
    tokio::spawn(async move {
        loop {
            // check if still running or if already all canceled
            let is_running = *sync_is_running_rx.borrow();
            if !is_running {
                break;
            }

            // TODO: ping from time to time the nodes i need to pull from
            //       connect to them, see if there is something new
            //       request blob ticket ids

            // wait 1 second for the next iteration
            sleep(Duration::from_millis(1000)).await;
        }
    });

    // need the loop on a separate thread so we can listen for the ctrl_c signal
    let kb_thread = tokio::spawn(async move {
        // TODO: if we do like this, signals are no longer accepted
        enable_raw_mode().unwrap();

        loop {
            let evt = event::read().unwrap().as_key_press_event();
            if evt.is_none() {
                continue;
            }

            // check for keyboard events
            let key_event = evt.unwrap();
            let signal_exit =
                event::KeyEvent::new(event::KeyCode::Char('c'), event::KeyModifiers::CONTROL);
            if key_event == signal_exit || key_event.code == event::KeyCode::Char('q') {
                sync_is_running_tx.send(false).unwrap();
                break;
            } else if key_event.code == event::KeyCode::Char('r') {
                // TODO: cant do like this otherwise config moves
                config.reload_filesyncs();
                config.reload_trustees();
            }
        }

        disable_raw_mode().unwrap();
    });

    // wait for all the keyboard events
    // included will be the signal exit
    kb_thread.await.unwrap();

    println!("closing");
    conn.close().await?;

    Ok(())
}
