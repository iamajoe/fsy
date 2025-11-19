use std::fs;

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};

use crate::file_ledger_repository;

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

pub struct Local {
    is_lock_sync: bool,
    file_ledger_repo: file_ledger_repository::FileLedgerRepository,
    receive_evts: Vec<ReceiveFromData>,
}

impl Local {
    pub fn new(
        is_lock_sync: bool,
        file_ledger_repo: file_ledger_repository::FileLedgerRepository,
    ) -> Self {
        Self {
            is_lock_sync,
            file_ledger_repo,
            receive_evts: vec![],
        }
    }

    pub fn check_events(&mut self) -> Result<(Vec<SendToData>, Vec<ReceiveFromData>)> {
        let receive_evts = std::mem::take(&mut self.receive_evts);
        Ok((vec![], receive_evts))
    }

    pub async fn send_file(&mut self, data: SendToData) -> Result<()> {
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

    pub async fn receive_file(&self, data: ReceiveFromData, wait_unlock_ms: u64) -> Result<()> {
        // no point in updating a file that was already saved or is older
        if !super::should_file_update(
            &self.file_ledger_repo,
            &data.id,
            &data.src_relative,
            &data.timestamp,
        ) {
            return Ok(());
        }

        let full_dest = super::get_full_dest_path(&data.src_relative, &data.dest).unwrap();

        // lock file so that the watcher doesn't listen to changes
        super::lock_file(&self.file_ledger_repo, &full_dest).unwrap();

        // advance with the integration
        receive_file(&data.src_full, &full_dest).unwrap();

        // save the pull file so that when the same timestamp comes in,
        // we know if we have the right one or not
        self.file_ledger_repo
            .save_pull_file(&data.id, &data.src_relative, &data.timestamp)
            .unwrap();

        // we can unlock now
        super::unlock_file(
            &self.file_ledger_repo,
            &full_dest,
            wait_unlock_ms,
            self.is_lock_sync,
        )
        .await
        .unwrap();

        Ok(())
    }
}

fn receive_file(src_full: &str, dest: &str) -> Result<()> {
    if let Err(e) = fs::copy(src_full, dest) {
        return Err(anyhow!(e));
    }

    Ok(())
}
