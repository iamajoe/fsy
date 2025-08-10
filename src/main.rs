use std::error::Error;

mod config;
mod connection;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // TODO: error could be better
    let config = config::fetch_config().unwrap();
    dbg!(&config);

    let router = connection::start_accept_side().await?;
    let conn_data = connection::start_connect_side(&router).await?;
    connection::close_connect_side(conn_data).await?;

    connection::close_accept_side(router).await?;

    Ok(())
}
