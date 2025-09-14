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

        let conn = self
            .router
            .endpoint()
            .connect(node_addr, MESSAGE_PROTOCOL_ALPN)
            .await?;

        let (mut send, mut _recv) = conn.open_bi().await?;

        // TODO: send the actual bytes of the message
        let msg_bytes = msg.as_bytes();

        // send message
        println!("connected, writing all");
        send.write_all(b"open_conn").await?;
        send.finish()?;

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
        println!("accepting bi communication...");

        match connection.accept_bi().await {
            Ok((mut send, mut recv)) => {
                println!("copying bytes...");
                let bytes_sent = tokio::io::copy(&mut recv, &mut send).await?;

                println!("reading bytes...");
                // TODO: how to convert bytes_sent? how are we sure it is a string?
                let response = recv.read_to_end(bytes_sent as usize).await.unwrap();
                println!("- Response: {:?}", String::from_utf8(response));

                let node_id = connection.remote_node_id()?;
                // TODO: send to the message watcher
                let _ = self
                    .message_watcher_tx
                    .send(Some((node_id.to_string(), String::from("MESSAGE"))));

                send.finish()?;
                connection.closed().await;

                Ok(())
            },
            Err(e) => {
                println!("error accepting bi: {e}");
                Err(AcceptError::from_err(e))
            }
        }
    }
}
