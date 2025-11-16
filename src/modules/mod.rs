mod local;

use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};
use std::time::Duration;
use tokio::time::sleep;

use crate::file_ledger_repository::FileLedgerRepository;

pub enum TargetKind {
    // id, src_full, src_relative, dest, timestamp
    Local(String, String, String, String, DateTime<Utc>),
}

pub struct TargetKindModules {
    // TODO: set p2p connection as option
    file_ledger_repo: FileLedgerRepository,
    is_lock_sync: bool,
}

impl TargetKindModules {
    pub fn new(file_ledger_repo: FileLedgerRepository) -> Self {
        Self {
            file_ledger_repo,
            is_lock_sync: false,
        }
    }

    pub async fn send_target(
        &self,
        kind: TargetKind,
        wait_unlock_ms: u64,
    ) -> Result<()> {
        match kind {
            TargetKind::Local(id, src_full, src_relative, dest, timestamp) => {
                // NOTE: local is a special case where we can send directly
                return self
                    .receive_target(
                        TargetKind::Local(id, src_full, src_relative, dest, timestamp),
                        wait_unlock_ms,
                    )
                    .await;
            }
        }
    }

    pub async fn receive_target(
        &self,
        kind: TargetKind,
        wait_unlock_ms: u64,
    ) -> Result<()> {
        match kind {
            TargetKind::Local(id, src_full, src_relative, dest, timestamp) => {
                // no point in updating a file that was already saved or is older
                if let Ok(is) =
                    self.file_ledger_repo
                        .is_pull_file_updated(&id, &src_relative, &timestamp)
                    && is
                {
                    return Ok(());
                }

                let full_dest = get_full_dest_path(&src_relative, &dest).unwrap();

                // lock file so that the watcher doesn't listen to changes
                println!("[modules][lock file] {full_dest}");
                self.file_ledger_repo.lock_file(&full_dest).unwrap();

                local::receive_file(&src_full, &full_dest).unwrap();

                // save the pull file so that when the same timestamp comes in,
                // we know if we have the right one or not
                self.file_ledger_repo
                    .save_pull_file(&id, &src_relative, &timestamp)
                    .unwrap();

                // need to debounce as per the watcher debounce that can
                // come through still
                // we don't want to hinder the main system because of it though
                let file_ledger_repo = self.file_ledger_repo.clone();

                // the file watcher has a debounce for file change
                // the locking mechanism exists to prevent that change event
                // to trigger and loop. as such, we want to make sure we unlock
                // only after the debounce is done
                // NOTE: we want to use the lock sync for systems with less cores
                //       or for example for testing
                let wait_unlock_ms  = wait_unlock_ms + 500;
                if self.is_lock_sync {
                    sleep(Duration::from_millis(wait_unlock_ms)).await;
                    println!("[modules][unlock file] {full_dest} {wait_unlock_ms}ms");
                    file_ledger_repo.unlock_file(&full_dest).unwrap();
                } else {
                    tokio::spawn(async move {
                        sleep(Duration::from_millis(wait_unlock_ms)).await;
                        println!("[modules][unlock file] {full_dest} {wait_unlock_ms}ms");
                        file_ledger_repo.unlock_file(&full_dest).unwrap();
                    });
                }

                Ok(())
            }
        }
    }

    pub fn close(&self) -> Result<()> {
        // TODO: close p2p connection
        Ok(())
    }
}

fn get_full_dest_path(src_relative: &str, dest: &str) -> Result<String> {
    let mut dest_full = dest.to_owned();

    // empty relative means it is a file, not a directory
    // as a directory, we need to add the relative
    if !src_relative.is_empty() {
        let dest_full_raw = Path::new(dest).join(src_relative);
        dest_full = dest_full_raw.to_str().unwrap().to_owned();
    }

    Ok(dest_full)
}
