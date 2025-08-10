use std::error::Error;

use iroh::{
    Endpoint, Watcher,
    endpoint::{self},
    protocol::{self},
};

const ALPN: &[u8] = b"fsys/0";

pub async fn start_accept_side() -> Result<protocol::Router, endpoint::BindError> {
    let endpoint = Endpoint::builder().discovery_n0().bind().await?;
    let router = protocol::Router::builder(endpoint)
        .accept(ALPN, Echo)
        .spawn();

    Ok(router)
}

pub async fn close_accept_side(router: protocol::Router) -> Result<(), Box<dyn Error>> {
    router.shutdown().await?;

    Ok(())
}

pub async fn start_connect_side(
    router: &protocol::Router,
) -> Result<(Endpoint, endpoint::Connection), Box<dyn Error>> {
    let node_addr = router.endpoint().node_addr().initialized().await;
    let endpoint = Endpoint::builder().discovery_n0().bind().await?;
    let conn = endpoint.connect(node_addr, ALPN).await?;
    let (mut send, mut recv) = conn.open_bi().await?;

    send.write_all(b"Hello world").await?;
    send.finish()?;

    let response = recv.read_to_end(1000).await?;
    assert_eq!(&response, b"Hello world");

    Ok((endpoint, conn))
}

pub async fn close_connect_side(
    conn: (Endpoint, endpoint::Connection),
) -> Result<(), Box<dyn Error>> {
    let (endpoint, conn) = conn;

    conn.close(0u32.into(), b"bye!");

    endpoint.close().await;

    Ok(())
}

#[derive(Debug, Clone)]
struct Echo;

impl protocol::ProtocolHandler for Echo {
    async fn accept(&self, connection: endpoint::Connection) -> Result<(), protocol::AcceptError> {
        let node_id = connection.remote_node_id()?;
        println!("accepted connection from {node_id}");

        let (mut send, mut recv) = connection.accept_bi().await?;
        let bytes_sent = tokio::io::copy(&mut recv, &mut send).await?;
        println!("copied over {bytes_sent} byte(s)");

        send.finish()?;
        connection.closed().await;

        Ok(())
    }
}
