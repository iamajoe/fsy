pub mod connection;

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::core::tree;

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

pub struct RequestFileData {
    pub id: String,
    pub src_relative: String,
    pub node_id: String,
}

pub struct RequestTreeStatusData {
    pub id: String,
    pub node_id: String,
    pub tree: tree::Tree,
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

    pub fn get_evts_to_send(&mut self) -> Result<Vec<SendToData>> {
        // TODO: ..
        Ok(vec![])
    }

    pub fn get_evts_to_receive(&mut self) -> Result<Vec<ReceiveFromData>> {
        // TODO: ..
        Ok(vec![])
    }

    pub fn request_tree_status(&self, data: RequestTreeStatusData) -> Result<tree::Tree> {
        // TODO: ...
        todo!();
    }

    pub async fn send_file(&self, data: SendToData) -> Result<()> {
        todo!()
    }

    pub async fn receive_file(&self, data: ReceiveFromData, wait_unlock_ms: u64) -> Result<()> {
        todo!()
    }

    pub async fn request_file(&mut self, data: RequestFileData) -> Result<()> {
        todo!()
    }

    pub async fn close(&self) -> Result<()> {
        self.conn.close().await
    }
}
