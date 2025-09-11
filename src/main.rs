mod config;
mod connection;
mod error;
mod key;
mod sync_watcher;

use error::*;

#[tokio::main]
async fn main() -> Result<()> {
    // TODO: retrieve from cli, like --config "path"
    let user_relative_path = "";

    // listen to config changes so we can setup all the syncing processes
    let (syncs_is_running_tx, syncs_is_running_rx) = tokio::sync::watch::channel(true);
    tokio::spawn(async move {
        init_syncs(user_relative_path, syncs_is_running_rx)
            .await
            .unwrap();
    });

    // wait for all the keyboard events
    // included will be the signal exit
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for event");
    println!("closing");

    syncs_is_running_tx.send(false).unwrap();

    Ok(())
}

async fn init_syncs(
    user_relative_path: &'static str,
    is_running_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<()> {
    let config = config::Config::new(user_relative_path).unwrap();

    println!("setting storage tmp folder...");
    let tmp_dir = std::env::temp_dir().join("fsy_storage");
    std::fs::create_dir_all(&tmp_dir).unwrap();

    println!("opening node...");
    let mut conn = connection::Connection::new(&config.local.secret_key, &tmp_dir).await?;
    let node_id = conn.get_node_id();
    println!("waiting for requests. public id: {node_id}");

    let mut watcher =
        sync_watcher::SyncWatcher::new(&conn, &config.file_syncs, &config.trustees, config.local.push_debounce_secs).unwrap();

    // start the watcher
    watcher.start(is_running_rx).await;

    println!("shutting down the watcher");

    // watch was a blocker, as such, right now, we can close all senders
    watcher.close().unwrap();

    println!("shutting down the connection");

    // close the connection
    conn.close().await?;

    Ok(())
}
