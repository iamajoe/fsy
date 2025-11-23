use std::fs;

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};

use crate::core::tree;

pub struct ReceiveFromData {
    pub id: String,
    pub src_full: String,
    pub src_relative: String,
    pub dest: String,
    pub timestamp: DateTime<Utc>,
}

pub struct SendToData {
    pub id: String,
    pub src_full: String,
    pub src_relative: String,
    pub dest: String,
    pub timestamp: DateTime<Utc>,
}

pub struct RequestFileData {
    pub id: String,
    pub src_relative: String,
}

pub struct RequestTreeStatusData {
    pub id: String,
    pub tree: tree::Tree,
}

pub struct Local {
    receive_evts: Vec<ReceiveFromData>,
}

impl Local {
    pub fn new() -> Self {
        Self {
            receive_evts: vec![],
        }
    }

    pub fn get_evts_to_send(&mut self) -> Result<Vec<SendToData>> {
        // NOTE: local is a special case where we can send directly
        Ok(vec![])
    }

    pub fn get_evts_to_receive(&mut self) -> Result<Vec<ReceiveFromData>> {
        let receive_evts = std::mem::take(&mut self.receive_evts);
        Ok(receive_evts)
    }

    pub fn request_tree_status(&self, data: RequestTreeStatusData) -> Result<tree::Tree> {
        // TODO: ...
        todo!();
    }

    pub fn send_file(&mut self, data: SendToData) -> Result<()> {
        // NOTE: local is a special case where we can send directly
        self.receive_evts.push(ReceiveFromData {
            id: data.id,
            src_full: data.src_full,
            src_relative: data.src_relative,
            dest: data.dest,
            timestamp: data.timestamp,
        });

        Ok(())
    }

    pub fn receive_file(
        &self,
        data: ReceiveFromData,
        full_dest: String,
    ) -> Result<()> {
        if let Err(e) = fs::copy(&data.src_full, &full_dest) {
            return Err(anyhow!(e));
        }

        Ok(())
    }

    pub fn request_file(&mut self, data: RequestFileData) -> Result<()> {
        // TODO: ...
        Ok(())
    }
}
