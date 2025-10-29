use anyhow::Result;
use iroh::{
    Endpoint, NodeAddr, NodeId, SecretKey, Watcher,
    protocol::{self, AcceptError, ProtocolHandler},
};
use iroh_blobs::{store::{fs::FsStore, mem::MemStore}, ticket::BlobTicket, BlobsProtocol};
use std::{ path::{Path, PathBuf}, str::FromStr };
use tokio::sync::watch;

const MESSAGE_PROTOCOL_ALPN: &[u8] = b"iroh/ping/0";

#[derive(Debug, Clone)]
pub enum ConnEvent {
    // node_id, raw_msg
    ReceivedMessage(String, String),
}

#[derive(Clone)]
pub struct Connection {
    router: protocol::Router,
    message_watcher_rx: watch::Receiver<Option<ConnEvent>>,
    // store: MemStore,
    store: FsStore,
}

impl Connection {
    pub async fn new(raw_secret_key: &[u8; 32], store_path: &Path) -> Result<Self> {
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
        let store = FsStore::load(store_path).await.unwrap();
        let blobs = BlobsProtocol::new(&store, endpoint.clone(), None);

        // TODO: how can i check for the allowed list?
        //       how do i know that the user can actually connect?
        let (message_watcher_tx, message_watcher_rx) = watch::channel(None);
        let message_protocol = MessageProtocol::new(message_watcher_tx);
        let router = protocol::Router::builder(endpoint.clone())
            .accept(iroh_blobs::ALPN, blobs.clone()) // TODO: will this work?!
            .accept(MESSAGE_PROTOCOL_ALPN, message_protocol)
            .spawn();

        // TODO: need some sort of protocol for communication so that
        //       node can request a file
        //       need also to check locally if it can pull
        //       if all good, creates a blob ticket for the other to
        //       download.

        Ok(Self {
            router,
            message_watcher_rx,
            store,
        })
    }

    pub fn get_node_id(&self) -> String {
        self.router.endpoint().node_id().to_string()
    }

    pub fn get_events(&mut self) -> Result<Option<ConnEvent>> {
        // only proceed if something has changed
        if !self.message_watcher_rx.has_changed().unwrap() {
            return Ok(None);
        }

        // check the changed data
        let watch_msg = self.message_watcher_rx.borrow_and_update().clone();
        Ok(watch_msg)
    }

    pub async fn send_msg_to_node(&self, node_id: String, msg: String) -> Result<()> {
        let node = NodeId::from_str(&node_id);
        let node_addr = NodeAddr::new(node.unwrap());

        // open a connection to the accepting node
        let conn = self
            .router
            .endpoint()
            .connect(node_addr, MESSAGE_PROTOCOL_ALPN)
            .await?;

        let (mut send, mut recv) = conn.open_bi().await?; // Open a bidirectional QUIC stream

        send.write_all(msg.as_bytes()).await?; // send message
        send.finish()?; // signal the end of data for this particular stream

        // wait for the ok
        let response = recv.read_to_end(2).await?;
        assert_eq!(&response, b"ok");

        // nothing else more to do in the connection.
        let close_msg = "bye";
        conn.close(0u32.into(), close_msg.as_bytes());

        Ok(())
    }

    pub async fn get_file_ticket(&self, file_path: String) -> Result<BlobTicket> {
        let filename: PathBuf = file_path.parse()?;
        let abs_path = std::path::absolute(&filename)?;
        let tag = self.store.blobs().add_path(abs_path).await?;
        let addr = self.router.endpoint().node_addr().initialized().await;
        let ticket = BlobTicket::new(addr, tag.hash, tag.format);

        Ok(ticket)
    }

    pub async fn download_ticket_to_path(&self, ticket_id: String, file_path: String) -> Result<()> {
        let filename: PathBuf = file_path.parse()?;
        let abs_path = std::path::absolute(filename)?;
        let ticket: BlobTicket = ticket_id.parse()?;

        let downloader = self.store.downloader(self.router.endpoint());
        downloader.download(ticket.hash(), Some(ticket.node_addr().node_id)).await?;
        // TODO: should return bytes instead
        self.store.blobs().export(ticket.hash(), abs_path).await?;

        // let connection = self
        //     .router
        //     .endpoint()
        //     .connect(ticket.node_addr().node_id, iroh_blobs::ALPN)
        //     .await?;
        // let mut progress = iroh_blobs::get::request::get_blob(connection, ticket.hash());
        // let _ = loop {
        //     match progress.next().await {
        //         Some(GetBlobItem::Item(item)) => match item {
        //             BaoContentItem::Leaf(leaf) => {
        //                 // TODO: we are not moving this yet because the file might be too big
        //                 //       and we don't want to move it on memory in that case, we 
        //                 //       want to stream it in
        //                 //       in that case, this write, might not work at all and maybe
        //                 //       we want to get back to the download but then, how do we handle
        //                 //       the progress?!
        //                 write(&abs_path, leaf.data).unwrap();
        //             }
        //             BaoContentItem::Parent(parent) => {
        //                 println!("Parent: {parent:?}");
        //             }
        //         },
        //         Some(GetBlobItem::Done(stats)) => {
        //             println!("Stats: {} {} {}", stats.mbits(), stats.payload_bytes_read, stats.total_bytes_read());
        //             break stats;
        //         }
        //         Some(GetBlobItem::Error(err)) => {
        //             anyhow::bail!("Error while streaming blob: {err}");
        //         }
        //         None => {
        //             anyhow::bail!("Stream ended unexpectedly.");
        //         }
        //     }
        // };

        // TODO: what about progress?!

        Ok(())

        // let (bytes, _stats) = progress.bytes_and_stats().await?;
        // Ok(bytes)
    }

    pub async fn close(&self) -> Result<()> {
        self.router.endpoint().close().await;
        self.router.shutdown().await?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct MessageProtocol {
    message_watcher_tx: watch::Sender<Option<ConnEvent>>,
}

impl MessageProtocol {
    pub fn new(watcher_tx: watch::Sender<Option<ConnEvent>>) -> Self {
        Self {
            message_watcher_tx: watcher_tx,
        }
    }
}

impl ProtocolHandler for MessageProtocol {
    async fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> std::result::Result<(), AcceptError> {
        let node_id = connection.remote_node_id()?;

        let (mut send, mut recv) = connection
            .accept_bi()
            .await
            .map_err(AcceptError::from_err)?;

        // read until the peer finishes the stream
        let res = recv
            .read_to_end(usize::MAX)
            .await
            .map_err(AcceptError::from_err)?;

        // send an ok message that arrived
        send.write_all(b"ok").await.map_err(AcceptError::from_err)?;
        send.finish()?;

        let res = String::from_utf8_lossy(&res);

        // wait until the remote closes the connection, which it does once it
        // received the response.
        connection.closed().await;

        let evt = ConnEvent::ReceivedMessage(node_id.to_string(), res.to_string());
        let _ = self.message_watcher_tx.send(Some(evt));

        Ok(())
    }
}
