pub mod local;
pub mod p2p;

use std::path::Path;

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use std::time::Duration;
use tokio::time::sleep;

use crate::file_ledger_repository::FileLedgerRepository;

pub enum IntegrationToKind {
    Local(local::SendToData),
    P2p(p2p::SendToData),
}

pub enum IntegrationFromKind {
    Local(local::ReceiveFromData),
    P2p(p2p::ReceiveFromData),
}

pub struct Integrations {
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
            int_local: local::Local::new(false, file_ledger_repo.clone()),
            int_p2p,
        })
    }

    pub async fn check_events(
        &mut self,
    ) -> Result<(
        Vec<(String, IntegrationToKind)>,
        Vec<(String, IntegrationFromKind)>,
    )> {
        let mut to_arr: Vec<(String, IntegrationToKind)> = vec![];
        let mut from_arr: Vec<(String, IntegrationFromKind)> = vec![];

        // handle local
        match self.int_local.check_events() {
            Ok((send_to, receive_from)) => {
                for to in send_to {
                    to_arr.push((to.id.clone(), IntegrationToKind::Local(to)));
                }

                for from in receive_from {
                    from_arr.push((from.id.clone(), IntegrationFromKind::Local(from)));
                }
            }
            Err(e) => {
                return Err(anyhow!(e));
            }
        }

        // handle p2p
        if let Some(p2p) = &mut self.int_p2p {
            match p2p.check_events() {
                Ok((send_to, receive_from)) => {
                    for to in send_to {
                        to_arr.push((to.id.clone(), IntegrationToKind::P2p(to)));
                    }

                    for from in receive_from {
                        from_arr.push((from.id.clone(), IntegrationFromKind::P2p(from)));
                    }
                }
                Err(e) => {
                    return Err(anyhow!(e));
                }
            }
        }

        Ok((to_arr, from_arr))
    }

    pub async fn send_file(&mut self, kind: IntegrationToKind) -> Result<()> {
        match kind {
            IntegrationToKind::Local(data) => {
                return self.int_local.send_file(data).await;
            }

            IntegrationToKind::P2p(data) => {
                if let Some(p2p) = &self.int_p2p {
                    return p2p.send_file(data).await;
                }

                Ok(())
            }
        }
    }

    pub async fn receive_file(&self, kind: IntegrationFromKind, wait_unlock_ms: u64) -> Result<()> {
        match kind {
            IntegrationFromKind::Local(data) => {
                return self.int_local.receive_file(data, wait_unlock_ms).await;
            }

            IntegrationFromKind::P2p(data) => {
                if let Some(p2p) = &self.int_p2p {
                    return p2p.receive_file(data, wait_unlock_ms).await;
                }

                Ok(())
            }
        }
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
    id: &str,
    src_relative: &str,
    timestamp: &DateTime<Utc>,
) -> bool {
    if let Ok(is) = file_ledger_repo.is_pull_file_updated(id, src_relative, timestamp)
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
