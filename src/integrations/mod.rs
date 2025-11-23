pub mod local;
pub mod p2p;

use std::path::Path;

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use std::time::Duration;
use tokio::time::sleep;

use crate::core::file_ledger_repository::LedgerFileSave;
use crate::core::tree;
use crate::file_ledger_repository::{FileLedgerRepository, LedgerFile};

pub enum SendToKind {
    Local(local::SendToData),
    P2p(p2p::SendToData),
}

pub enum ReceiveFromKind {
    Local(local::ReceiveFromData),
    P2p(p2p::ReceiveFromData),
}

pub enum RequestFileKind {
    Local(local::RequestFileData),
    P2p(p2p::RequestFileData),
}

pub enum RequestTreeStatusKind {
    Local(local::RequestTreeStatusData),
    P2p(p2p::RequestTreeStatusData),
}

pub struct Integrations {
    file_ledger_repo: FileLedgerRepository,
    int_local: local::Local,
    int_p2p: Option<p2p::P2p>,
}

impl Integrations {
    pub async fn new(
        file_ledger_repo: FileLedgerRepository,
        connect_p2p: bool,
        p2p_secret_key: &[u8; 32],
    ) -> Result<Self> {
        let mut int_p2p: Option<p2p::P2p> = None;
        if connect_p2p {
            int_p2p = Some(p2p::P2p::new(p2p_secret_key).await.unwrap());
        }

        Ok(Self {
            int_local: local::Local::new(),
            int_p2p,
            file_ledger_repo,
        })
    }

    pub async fn get_evts_to_send(&mut self) -> Result<Vec<(String, SendToKind)>> {
        let mut arr: Vec<(String, SendToKind)> = vec![];

        // handle local
        match self.int_local.get_evts_to_send() {
            Ok(send_to) => {
                for to in send_to {
                    arr.push((to.id.clone(), SendToKind::Local(to)));
                }
            }
            Err(e) => {
                return Err(anyhow!(e));
            }
        }

        // handle p2p
        if let Some(p2p) = &mut self.int_p2p {
            // TODO: should probably setup a queue for receiving messages
            //       and set the remaining on a different thread. if there is a file of
            //       1gb, we don't want to wait for it
            match p2p.get_evts_to_send() {
                Ok(send_to) => {
                    for to in send_to {
                        arr.push((to.id.clone(), SendToKind::P2p(to)));
                    }
                }
                Err(e) => {
                    return Err(anyhow!(e));
                }
            }
        }

        Ok(arr)
    }

    pub async fn get_evts_to_receive(&mut self) -> Result<Vec<(String, ReceiveFromKind)>> {
        let mut arr: Vec<(String, ReceiveFromKind)> = vec![];

        // handle local
        match self.int_local.get_evts_to_receive() {
            Ok(send_to) => {
                for to in send_to {
                    arr.push((to.id.clone(), ReceiveFromKind::Local(to)));
                }
            }
            Err(e) => {
                return Err(anyhow!(e));
            }
        }

        // handle p2p
        if let Some(p2p) = &mut self.int_p2p {
            // TODO: should probably setup a queue for receiving messages
            //       and set the remaining on a different thread. if there is a file of
            //       1gb, we don't want to wait for it
            match p2p.get_evts_to_receive() {
                Ok(send_to) => {
                    for to in send_to {
                        arr.push((to.id.clone(), ReceiveFromKind::P2p(to)));
                    }
                }
                Err(e) => {
                    return Err(anyhow!(e));
                }
            }
        }

        Ok(arr)
    }

    pub async fn request_tree_status(&self, kind: RequestTreeStatusKind) -> Result<tree::Tree> {
        match kind {
            RequestTreeStatusKind::Local(data) => self.int_local.request_tree_status(data),

            RequestTreeStatusKind::P2p(data) => {
                if let Some(p2p) = &self.int_p2p {
                    return p2p.request_tree_status(data);
                }

                todo!()
            }
        }
    }

    pub async fn send_files(&mut self, kinds: Vec<SendToKind>) -> Result<()> {
        // TODO: if we are sending we should also save the fingerprint

        // TODO: should handle bulks internally to the modules
        for kind in kinds {
            match kind {
                SendToKind::Local(data) => {
                    // TODO: handle error
                    self.int_local.send_file(data).unwrap();
                }

                SendToKind::P2p(data) => {
                    if let Some(p2p) = &self.int_p2p {
                        // TODO: handle error
                        p2p.send_file(data).await.unwrap();
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn receive_files(
        &self,
        kinds: Vec<(ReceiveFromKind, u64)>, // u64 will be debounce time
    ) -> Result<()> {
        // TODO: should handle bulks internally to the modules
        for (kind, wait_unlock_ms) in kinds {
            let mut locked_src: Option<String> = None;
            let mut dest_src: Option<String> = None;

            match kind {
                ReceiveFromKind::Local(data) => {
                    // no point in updating a file that was already saved or is older
                    if !should_file_update(&self.file_ledger_repo, &data.src_full, &data.timestamp)
                    {
                        continue;
                    }

                    // lock the file before receiving, we don't want to trigger file changes
                    let full_dest = get_full_dest_path(&data.src_relative, &data.dest).unwrap();
                    lock_file(&self.file_ledger_repo, &full_dest).unwrap();
                    locked_src = Some(full_dest.clone());

                    // TODO: handle error
                    self.int_local.receive_file(data, full_dest.clone()).unwrap();

                    dest_src = Some(full_dest);
                }

                ReceiveFromKind::P2p(data) => {
                    // TODO: a lot to handle here!

                    if let Some(p2p) = &self.int_p2p {
                        // TODO: handle error
                        p2p.receive_file(data, wait_unlock_ms).await.unwrap();
                    }
                }
            }

            // fingerprint the file and save it for later
            if let Some(src) = dest_src {
                // save the file so that when the same timestamp comes in,
                // we know if we have the right one or not
                // TODO: should be on parent
                let finger = tree::fingerprint_file(&src).unwrap();
                self.file_ledger_repo
                    .save_file(LedgerFileSave {
                        file_path: src,
                        fingerprint: finger,
                    })
                    .unwrap();
            }

            // proceed with the unlock
            if let Some(src) = locked_src {
                // we can unlock now
                unlock_file(
                    &self.file_ledger_repo,
                    &src,
                    wait_unlock_ms,
                    false,
                )
                .await
                .unwrap();
            }
        }

        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        if let Some(p2p) = &self.int_p2p {
            return p2p.close().await;
        }

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

fn should_file_update(
    file_ledger_repo: &FileLedgerRepository,
    src: &str,
    timestamp: &DateTime<Utc>,
) -> bool {
    let finger = tree::fingerprint_file(src).unwrap();
    let is = file_ledger_repo.is_file_sync(LedgerFile {
        file_path: src.to_owned(),
        fingerprint: finger,
        lock_count: 0,
        updated_at: timestamp.to_owned(),
    });

    if let Ok(is) = is
        && is
    {
        return true;
    }

    // default is that the file should update
    true
}

fn lock_file(file_ledger_repo: &FileLedgerRepository, full_dest: &str) -> Result<()> {
    // lock file so that the watcher doesn't listen to changes
    println!("[modules][lock file] {full_dest}");
    file_ledger_repo.lock_file(full_dest).unwrap();

    Ok(())
}

async fn unlock_file(
    file_ledger_repo: &FileLedgerRepository,
    full_dest: &str,
    wait_unlock_ms: u64,
    is_lock_sync: bool,
) -> Result<()> {
    let full_dest = full_dest.to_owned();

    // need to debounce as per the watcher debounce that can
    // come through still
    // we don't want to hinder the main system because of it though
    let file_ledger_repo = file_ledger_repo.to_owned();

    // the file watcher has a debounce for file change
    // the locking mechanism exists to prevent that change event
    // to trigger and loop. as such, we want to make sure we unlock
    // only after the debounce is done
    // NOTE: we want to use the lock sync for systems with less cores
    //       or for example for testing
    let wait_unlock_ms = wait_unlock_ms + 500;
    if is_lock_sync {
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
