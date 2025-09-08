use std::{path::Path, str::FromStr, sync::Arc};

use crate::*;

use iroh::{
    Endpoint, NodeAddr, NodeId, SecretKey,
    protocol::{self, AcceptError, ProtocolHandler},
};
use iroh_blobs::ALPN;

pub struct Connection {
    is_open: bool,
    router: protocol::Router,
}

impl Connection {
    pub async fn new(raw_secret_key: &[u8; 32], _store_path: &Path) -> Result<Self> {
        let secret_key = SecretKey::from_bytes(raw_secret_key);

        let endpoint = Endpoint::builder()
            .secret_key(secret_key)
            // TODO: what about discovery over custom relay and local?
            .discovery_n0()
            // TODO: local is not working
            // .add_discovery(discovery::mdns::MdnsDiscovery::builder())
            .bind()
            .await
            .unwrap();

        // setup the protocol for the blobs back and forth
        // should use a file system on temporary dir
        // sending a file with gbs will fill up the ram and crash
        // let store = MemStore::new();
        // let store = FsStore::load(store_path).await.unwrap();
        // let blobs = BlobsProtocol::new(&store, endpoint.clone(), None);

        // TODO: how can i check for the allowed list?
        //       how do i know that the user can actually connect?
        let router = protocol::Router::builder(endpoint.clone())
            // .accept(iroh_blobs::ALPN, blobs)
            .accept(iroh_blobs::ALPN, Arc::new(ProtocolEcho))
            .spawn();

        // TODO: need some sort of protocol for communication so that
        //       node can request a file
        //       need also to check locally if it can pull
        //       if all good, creates a blob ticket for the other to
        //       download.

        Ok(Self {
            is_open: true,
            router,
        })
    }

    pub fn get_node_id(&self) -> String {
        self.router.endpoint().node_id().to_string()
    }

    pub async fn _connect_to_node(&self, node_id: &str) -> Result<()> {
        let node = NodeId::from_str(node_id);
        let node_addr = NodeAddr::new(node.unwrap());

        let conn = self
            .router
            .endpoint()
            .connect(node_addr, ALPN)
            .await
            .unwrap();
        let (mut send, mut recv) = conn.open_bi().await.unwrap();

        // test connection
        send.write_all(b"open_conn").await.unwrap();
        send.finish().unwrap();

        let response = recv.read_to_end(1000).await.unwrap();
        assert_eq!(&response, b"open_conn");

        // after all done, close client
        conn.close(0u32.into(), b"close_request");

        Ok(())
    }

    pub async fn send_msg_to_node(&self, node_id: &str, _msg: &str) -> Result<()> {
        let node = NodeId::from_str(node_id);
        let node_addr = NodeAddr::new(node.unwrap());

        let conn = self
            .router
            .endpoint()
            .connect(node_addr, ALPN)
            .await
            .unwrap();
        let (mut send, mut _recv) = conn.open_bi().await.unwrap();

        // test connection
        send.write_all(b"open_conn").await.unwrap();
        send.finish().unwrap();

        // TODO: we can use this for example to check if we should sync or not
        // let response = recv.read_to_end(1000).await.unwrap();
        // assert_eq!(&response, b"open_conn");

        // after all done, close client
        conn.close(0u32.into(), b"close_request");

        Ok(())
    }

    pub async fn close(&mut self) -> Result<()> {
        if !self.is_open {
            return Ok(());
        }

        self.is_open = false;

        self.router.endpoint().close().await;
        self.router.shutdown().await.unwrap();

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct ProtocolEcho;

impl ProtocolHandler for ProtocolEcho {
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> std::result::Result<(), AcceptError> {
        let (mut send, mut recv) = connection.accept_bi().await.unwrap();
        let bytes_sent = tokio::io::copy(&mut recv, &mut send).await.unwrap();
        dbg!(bytes_sent);

        send.finish().unwrap();
        connection.closed().await;

        Ok(())
    }
}
