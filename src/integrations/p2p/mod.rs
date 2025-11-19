pub mod connection;

use anyhow::Result;
use chrono::{DateTime, Utc};

pub struct ReceiveFromData {
    pub id: String,
    pub src_full: String,
    pub src_relative: String,
    pub node_id: String,
    pub dest_id: String,
    pub timestamp: DateTime<Utc>,
}

pub struct SendToData {
    pub id: String,
    pub src_full: String,
    pub src_relative: String,
    pub node_id: String,
    pub dest_id: String,
    pub timestamp: DateTime<Utc>,
}

pub struct P2p {
    conn: connection::Connection,
}

impl P2p {
    pub async fn new(secret_key: &[u8; 32]) -> Result<Self> {
        let tmp_dir = std::env::temp_dir().join("fsy_storage_p2p");
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let conn = connection::Connection::new(secret_key, &tmp_dir)
            .await
            .unwrap();

        Ok(Self { conn })
    }

    pub fn check_events(&mut self) -> Result<(Vec<SendToData>, Vec<ReceiveFromData>)> {
        let evts = &self.conn.get_events().unwrap();
        if evts.is_empty() {
            return Ok((vec![], vec![]));
        }

        let mut to_data: Vec<SendToData> = vec![];
        let mut from_data: Vec<ReceiveFromData> = vec![];

        for evt in evts {
            println!("[integrations][p2p][check_events] received event to convert");
            if let connection::ConnEvent::ReceivedMessage(node_id, _raw_msg) = evt {
                println!("[integrations][p2p][check_events] message received: {node_id}");

                // TODO: need to convert to an integration kind
                // let action = action::CommAction::from_namespaced_msg(&node_id, &raw_msg);

                // TODO: send it to receive
                // TODO: should the receive go through a different channel?
            }
        }

        Ok((to_data, from_data))
    }

    pub async fn send_file(&self, data: SendToData) -> Result<()> {
        todo!()
    }

    pub async fn receive_file(&self, data: ReceiveFromData, wait_unlock_ms: u64) -> Result<()> {
        todo!()
    }

    pub async fn close(&self) -> Result<()> {
        self.conn.close().await
    }
}

