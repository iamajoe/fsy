mod config;
mod connection;
mod error;
mod key;

use error::*;

#[tokio::main]
async fn main() -> Result<()> {
    // REF: https://github.com/n0-computer/iroh-blobs/blob/main/examples/transfer.rs

    let user_relative_path = "";
    // TODO: error could be better
    let config = config::Config::new(user_relative_path).unwrap();
    dbg!(&config);

    println!("setting storage tmp folder...");
    let tmp_dir = std::env::temp_dir().join("fsy_storage");
    std::fs::create_dir_all(&tmp_dir).unwrap();

    println!("opening node...");
    let mut conn = connection::Connection::new(&config.local.secret_key, &tmp_dir).await?;
    let node_id = conn.get_node_id();
    println!("waiting for requests. public id: {node_id}");

    if let Err(e) = tokio::signal::ctrl_c().await {
        dbg!(e);
        panic!("something went wrong");
    }

    println!("closing");
    conn.close().await?;

    Ok(())
}
