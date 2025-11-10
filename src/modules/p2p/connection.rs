use anyhow::Result;
use async_trait::async_trait;
use iroh::{
    Endpoint, NodeAddr, NodeId, SecretKey, Watcher,
    protocol::{self, AcceptError, ProtocolHandler},
};
use iroh_blobs::{BlobsProtocol, store::fs::FsStore, ticket::BlobTicket};
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};
use tokio::sync::mpsc;

const MESSAGE_PROTOCOL_ALPN: &[u8] = b"iroh/ping/0";

#[derive(Debug)]
pub enum ConnEvent {
    // node_id, raw_msg
    ReceivedMessage(String, String),

    // node_id, msg
    SendMessageToNode(String, String),

    // file_path
    GetFileTicket(String),

    // ticket_id, file_path
    DownloadTicketToPath(String, String),
}

#[derive(Debug, Clone)]
pub enum ConnEventResult {
    // ticket_id
    GetFileTicket(String),

    DownloadTicketToPath,
}

pub struct Connection {
    router: protocol::Router,
    message_ch_rx: mpsc::Receiver<ConnEvent>,
    // store: MemStore,
    store: FsStore,
}

#[async_trait]
trait ConnSendMessage {
    async fn send_msg_to_node(&self, node_id: String, msg: String) -> Result<()>;
}

#[async_trait]
trait ConnSendTicket {
    async fn get_file_ticket(&self, file_path: String) -> Result<BlobTicket>;
    async fn download_ticket_to_path(&self, ticket_id: String, file_path: String) -> Result<()>;
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
            .map_err(|e| anyhow::anyhow!("Endpoint.discovery_n0 failed: {e}"))?;

        // setup the protocol for the blobs back and forth
        // should use a file system on temporary dir
        // sending a file with gbs will fill up the ram and crash
        // let store = MemStore::new();
        let store = FsStore::load(store_path)
            .await
            .map_err(|e| anyhow::anyhow!("FsStore::load failed: {e}"))?;

        let blobs = BlobsProtocol::new(&store, endpoint.clone(), None);

        // TODO: how can i check for the allowed list?
        //       how do i know that the user can actually connect?
        let (message_ch_tx, message_ch_rx) = mpsc::channel(1000);
        let message_protocol = MessageProtocol::new(message_ch_tx);
        let router = protocol::Router::builder(endpoint.clone())
            .accept(iroh_blobs::ALPN, blobs.clone())
            .accept(MESSAGE_PROTOCOL_ALPN, message_protocol)
            .spawn();

        // TODO: need some sort of protocol for communication so that
        //       node can request a file
        //       need also to check locally if it can pull
        //       if all good, creates a blob ticket for the other to
        //       download.

        Ok(Self {
            router,
            message_ch_rx,
            store,
        })
    }

    pub fn get_node_id(&self) -> String {
        self.router.endpoint().node_id().to_string()
    }

    pub fn get_events(&mut self) -> Result<Vec<ConnEvent>> {
        let mut evts: Vec<ConnEvent> = vec![];
        while let Ok(msg) = self.message_ch_rx.try_recv() {
            evts.push(msg);
        }

        Ok(evts)
    }

    pub async fn close(&self) -> Result<()> {
        self.router.endpoint().close().await;
        self.router.shutdown().await?;

        Ok(())
    }
}

#[async_trait]
impl ConnSendMessage for Connection {
    async fn send_msg_to_node(&self, node_id: String, msg: String) -> Result<()> {
        let node = NodeId::from_str(&node_id)
            .map_err(|e| anyhow::anyhow!("NodeId::from_str(&node_id) failed: {e}"))?;
        let node_addr = NodeAddr::new(node);

        // open a connection to the accepting node
        let conn = self
            .router
            .endpoint()
            .connect(node_addr, MESSAGE_PROTOCOL_ALPN)
            .await
            .map_err(|e| anyhow::anyhow!("router.endpoint().connect() failed: {e}"))?;

        // Open a bidirectional QUIC stream
        let (mut send, mut recv) = conn
            .open_bi()
            .await
            .map_err(|e| anyhow::anyhow!("conn.open_bi() failed: {e}"))?;

        // send message
        send.write_all(msg.as_bytes())
            .await
            .map_err(|e| anyhow::anyhow!("send.write_all(msg.as_bytes()) failed: {e}"))?;

        // signal the end of data for this particular stream
        send.finish()
            .map_err(|e| anyhow::anyhow!("send.finish() failed: {e}"))?;

        // wait for the ok
        let response = recv.read_to_end(2).await?;
        assert_eq!(&response, b"ok");

        // nothing else more to do in the connection.
        let close_msg = "bye";
        conn.close(0u32.into(), close_msg.as_bytes());

        Ok(())
    }
}

#[async_trait]
impl ConnSendTicket for Connection {
    async fn get_file_ticket(&self, file_path: String) -> Result<BlobTicket> {
        let filename: PathBuf = file_path
            .parse()
            .map_err(|e| anyhow::anyhow!("file_path.parse() failed: {e}"))?;

        let abs_path = std::path::absolute(&filename)?;
        let tag = self
            .store
            .blobs()
            .add_path(abs_path)
            .await
            .map_err(|e| anyhow::anyhow!("store.blobs().add_path failed: {e}"))?;

        let addr = self.router.endpoint().node_addr().initialized().await;
        let ticket = BlobTicket::new(addr, tag.hash, tag.format);

        Ok(ticket)
    }

    async fn download_ticket_to_path(&self, ticket_id: String, file_path: String) -> Result<()> {
        let filename: PathBuf = file_path.parse()?;
        let abs_path = std::path::absolute(filename)?;
        let ticket: BlobTicket = ticket_id.parse()?;

        let downloader = self.store.downloader(self.router.endpoint());
        downloader
            .download(ticket.hash(), Some(ticket.node_addr().node_id))
            .await
            .map_err(|e| anyhow::anyhow!("downloader.download failed: {e}"))?;

        // TODO: should return bytes instead
        self.store
            .blobs()
            .export(ticket.hash(), abs_path)
            .await
            .map_err(|e| anyhow::anyhow!("store.blobs().export failed: {e}"))?;

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
}

#[derive(Debug)]
struct MessageProtocol {
    message_ch_tx: mpsc::Sender<ConnEvent>,
}

impl MessageProtocol {
    pub fn new(watcher_tx: mpsc::Sender<ConnEvent>) -> Self {
        Self {
            message_ch_tx: watcher_tx,
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
        self.message_ch_tx
            .send(evt)
            .await
            .map_err(AcceptError::from_err)?;

        Ok(())
    }
}

pub async fn process_conn_event(
    event: ConnEvent,
    conn: &Connection,
) -> Result<Option<ConnEventResult>> {
    match event {
        ConnEvent::SendMessageToNode(node_id, msg) => {
            conn.send_msg_to_node(node_id, msg)
                .await
                .map_err(|e| anyhow::anyhow!("send_msg_to_node failed: {e}"))?;
        }

        ConnEvent::GetFileTicket(file_path) => {
            let ticket_id = conn
                .get_file_ticket(file_path)
                .await
                .map_err(|e| anyhow::anyhow!("get_file_ticket failed: {e}"))?;

            // NOTE: special case where we want the data to be retrieved
            return Ok(Some(ConnEventResult::GetFileTicket(ticket_id.to_string())));
        }

        ConnEvent::DownloadTicketToPath(ticket_id, file_path) => {
            conn.download_ticket_to_path(ticket_id, file_path)
                .await
                .map_err(|e| anyhow::anyhow!("download_ticket_to_path failed: {e}"))?;

            // NOTE: special case where we want to be informed of when done
            return Ok(Some(ConnEventResult::DownloadTicketToPath));
        }

        _ => {}
    }

    Ok(None)
}
