use iroh::{
    Endpoint, NodeAddr, NodeId, SecretKey,
    protocol::{self, AcceptError, ProtocolHandler},
};
use std::{error::Error, path::Path, str::FromStr};
use tokio::{
    select,
    sync::watch::{Receiver, Sender, channel},
};

const MESSAGE_PROTOCOL_ALPN: &[u8] = b"iroh/ping/0";

pub struct Connection {
    router: protocol::Router,
    message_watcher_rx: Receiver<Option<(String, String)>>,
    message_sub_tx: Option<Sender<Option<(String, String)>>>,
}

impl Connection {
    pub async fn new(
        raw_secret_key: &[u8; 32],
        _store_path: &Path,
    ) -> Result<Self, Box<dyn Error>> {
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
        let (message_watcher_tx, message_watcher_rx) = channel::<Option<(String, String)>>(None);
        let message_protocol = MessageProtocol::new(message_watcher_tx);
        let router = protocol::Router::builder(endpoint.clone())
            // .accept(iroh_blobs::ALPN, blobs)
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
            message_sub_tx: None,
        })
    }

    pub fn get_node_id(&self) -> String {
        self.router.endpoint().node_id().to_string()
    }

    pub async fn send_msg_to_node(&self, node_id: &str, msg: &str) -> Result<(), Box<dyn Error>> {
        println!("sending message to node: {node_id} {msg}");

        let node = NodeId::from_str(node_id);
        let node_addr = NodeAddr::new(node.unwrap());

        // Open a connection to the accepting node
        let conn = self
            .router
            .endpoint()
            .connect(node_addr, MESSAGE_PROTOCOL_ALPN)
            .await?;

        let (mut send, mut recv) = conn.open_bi().await?; // Open a bidirectional QUIC stream

        println!("connected, writing all");
        send.write_all(msg.as_bytes()).await?; // send message
        println!("close connection");
        send.finish()?; // signal the end of data for this particular stream

        // wait for the ok
        let response = recv.read_to_end(2).await?;
        assert_eq!(&response, b"ok");

        // nothing else more to do in the connection.
        conn.close(0u32.into(), b"bye");

        Ok(())
    }

    // NOTE: function first argument should be node_id, second, message
    pub fn subscribe_to_msgs(&mut self, watcher_tx: Sender<Option<(String, String)>>) {
        self.message_sub_tx = Some(watcher_tx);
    }

    // NOTE: function first argument should be node_id, second, message
    pub fn unsubscribe_from_msgs(&mut self) {
        self.message_sub_tx = None;
    }

    pub async fn download_ticket_id(&self, ticket_id: String) {
        // TODO: ...
    }

    // iterates through the watcher
    pub async fn listen_to_messages(&mut self, is_running_rx: &mut Receiver<bool>) {
        // maybe it is not running already
        if !*is_running_rx.borrow() {
            return;
        }

        loop {
            select! {
                // if not running, get out
                _ = is_running_rx.changed() => {
                    if !*is_running_rx.borrow() {
                        break;
                    }
                },

                // check for evts from message
                _ = self.message_watcher_rx.changed() => {
                    if let Some(sub_tx) = &self.message_sub_tx {
                        let message = self.message_watcher_rx.borrow().clone();
                        let _ = sub_tx.send(message);
                    }
                },
            }
        }
    }

    pub async fn close(&mut self) -> Result<(), Box<dyn Error>> {
        self.router.endpoint().close().await;
        self.router.shutdown().await?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct MessageProtocol {
    message_watcher_tx: Sender<Option<(String, String)>>,
}

impl MessageProtocol {
    pub fn new(watcher_tx: Sender<Option<(String, String)>>) -> Self {
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
        println!("accepted connection from {node_id}");

        match connection.accept_bi().await {
            Ok((mut send, mut recv)) => {
                println!("receiving bytes...");
                // read until the peer finishes the stream
                let res = recv.read_to_end(usize::MAX).await.map_err(AcceptError::from_err)?;

                // send an ok message that arrived
                send.write_all(b"ok").await.map_err(AcceptError::from_err)?;
                send.finish()?;

                let res = String::from_utf8_lossy(&res);
                println!("- Received: {:?}", &res);

                // wait until the remote closes the connection, which it does once it
                // received the response.
                connection.closed().await;

                let _ = self
                    .message_watcher_tx
                    .send(Some((node_id.to_string(), res.to_string())));

                Ok(())
            }
            Err(e) => {
                println!("error accepting bi: {e}");
                Err(AcceptError::from_err(e))
            }
        }
    }
}
