pub mod core;
pub mod integrations;

use std::time::Duration;

use anyhow::{Result, anyhow};
use tokio::sync::{mpsc, watch};
use tokio::time::sleep;

use core::{config, file_ledger_repository, path_watcher};

pub fn run_start_process(
    cnf: &config::Config,
    _file_repo: &file_ledger_repository::FileLedgerRepository,
    _changed_target_data_tx: mpsc::Sender<(String, path_watcher::ChangedTarget)>,
) -> Result<()> {
    println!("[start] starting");

    // TODO: pseudo code
    for target in cnf.targets.iter() {
        if !target.enable {
            continue;
        }

        println!("Target: {}", &target.id);
        let target_ignored = target.ignore_nested_files.clone().unwrap_or(vec![]);
        let tree = core::tree::Tree::from_path(&target.src, &target_ignored).unwrap();
        println!("{}", tree);

        // handle push targets
        if target.mode == config::TargetMode::Push || target.mode == config::TargetMode::PushPull {
            // TODO: go through each file and just say that the target has changed
            //       the system will handle on the other side if the timestamp needs
            //       to be updated or not
            // TODO: handle push
        }

        // handle pull targets
        if target.mode == config::TargetMode::Pull || target.mode == config::TargetMode::PushPull {
            // TODO: request target from every guy on the other side
            //       we can't use files because on the other side might be different
            //       we probably want to save the tree then
            // TODO: handle pull
        }
    }

    // iterate targets
    //   request from target integration the timestamp for bulk of files of the target
    //   iterate pull files
    //     check updated timestamp on repo and if should update
    //     if should update, request the integration for the file
    //   iterate push files
    //     send changed target for the system to inform

    Ok(())
}

pub fn run_cron_process(
    _is_running_rx: watch::Receiver<bool>,
    _cnf: &config::Config,
    _file_repo: &file_ledger_repository::FileLedgerRepository,
    _changed_target_data_tx: mpsc::Sender<(String, path_watcher::ChangedTarget)>,
) {
    println!("[cron] starting");
    // TODO: ...
    // TODO: it can use part of the start process
    // TODO: don't forget to check locks
}

pub async fn run_watch_process(
    is_running_rx: watch::Receiver<bool>,
    cnf: &config::Config,
    file_repo: &file_ledger_repository::FileLedgerRepository,
    changed_target_data_tx: mpsc::Sender<(String, path_watcher::ChangedTarget)>,
) {
    // create the watchers
    let watchers: Vec<(String, path_watcher::PathWatcher)> = cnf
        .targets
        .iter()
        .filter_map(|t| {
            if !t.enable
                || (t.mode != config::TargetMode::Push && t.mode != config::TargetMode::PushPull)
            {
                return None;
            }

            // check for ignored and retrieve all of the sources
            let target_ignored = t.ignore_nested_files.clone().unwrap_or(vec![]);
            let tree = core::tree::Tree::from_path(&t.src, &target_ignored).unwrap();
            let srcs = tree.to_paths();
            if srcs.is_empty() {
                return None;
            }

            // setup the watcher
            let path_watcher = path_watcher::PathWatcher::new(
                path_watcher::WatchTarget {
                    id: t.id.clone(),
                    srcs,
                },
                get_target_file_debounce_ms(t),
            )
            .unwrap();

            Some((t.id.clone(), path_watcher))
        })
        .collect();

    // loop through the possible changes incoming
    if watchers.is_empty() {
        return;
    }

    println!("[watch] looping");
    loop {
        if !*is_running_rx.borrow() {
            break;
        }

        // check all watcher to targets
        let changed_target_data: Vec<(String, path_watcher::ChangedTarget)> = watchers
            .iter()
            .filter_map(|(target_id, watcher)| {
                // check for changed targets
                if let Ok(Some(changed_target)) = watcher.get_changed_target() {
                    // file is locked so any changes should be disregarded
                    let is_locked = file_repo.is_file_locked(&changed_target.src).unwrap();
                    if is_locked {
                        return None;
                    }

                    return Some((target_id.to_owned(), changed_target));
                }

                None
            })
            .collect();

        // cache the target kind to be handled
        for changed_target in changed_target_data {
            if let Err(e) = changed_target_data_tx.send(changed_target).await {
                println!("Sending changed target to channel errored: {e}");
            }
        }

        sleep(Duration::from_millis(cnf.loop_sleep_time_ms)).await;
    }

    // close all the watchers
    for (_, mut watcher) in watchers {
        watcher.close().unwrap();
    }
}

pub async fn run_integrations_process(
    is_running_rx: watch::Receiver<bool>,
    cnf: &config::Config,
    file_repo: &file_ledger_repository::FileLedgerRepository,
    mut changed_target_data_rx: mpsc::Receiver<(String, path_watcher::ChangedTarget)>,
) {
    // NOTE: only enable p2p if there a target with that kind
    let has_p2p = cnf
        .targets
        .iter()
        .any(|t| t.enable && t.kind == config::TargetKind::P2p);

    let mut integrations_mod =
        integrations::Integrations::new(file_repo.to_owned(), has_p2p, &cnf.p2p_secret_key)
            .await
            .unwrap();

    loop {
        if !*is_running_rx.borrow() {
            break;
        }

        let mut send_to: Vec<(String, integrations::SendToKind)> = vec![];

        // handle the targets incoming from other threads
        while let Ok((target_id, changed_target)) = changed_target_data_rx.try_recv() {
            if let Some(target) = cnf.targets.iter().find(|t| t.id == target_id) {
                match changed_target_to_integration_kind(target, changed_target) {
                    Ok(Some(data)) => {
                        send_to.push(data);
                    }
                    Err(e) => {
                        // NOTE: we don't want to mess the process if an error comes in, keep doing it
                        println!("[integrations][change] error: {e}");
                    }
                    _ => {}
                }
            }
        }

        // handle integration events
        match integrations_mod.get_evts_to_send().await {
            Ok(mut arr) => {
                send_to.append(&mut arr);
            }
            Err(e) => {
                // NOTE: we don't want to mess the process if an error comes in, keep doing it
                println!("[integrations][get_evts_to_send] error: {e}");
            }
        }

        // handle the receivals
        let receive_from: Vec<(String, integrations::ReceiveFromKind)> =
            match integrations_mod.get_evts_to_receive().await {
                Ok(arr) => arr,
                Err(e) => {
                    // NOTE: we don't want to mess the process if an error comes in, keep doing it
                    println!("[integrations][get_evts_to_receive] error: {e}");
                    vec![]
                }
            };
        let receive_from_data: Vec<(integrations::ReceiveFromKind, u64)> = receive_from
            .into_iter()
            .filter_map(|(target_id, data)| {
                if let Some(target) = cnf.targets.iter().find(|t| t.id == target_id) {
                    if !target.enable
                        || (target.mode != config::TargetMode::Pull
                            && target.mode != config::TargetMode::PushPull)
                    {
                        return None;
                    }

                    let debounce = calc_target_unlock_file_debounce(cnf, target);
                    return Some((data, debounce));
                }

                None
            })
            .collect();
        if !receive_from_data.is_empty() {
            println!("[integrations][receive] start...");
            if let Err(e) = integrations_mod.receive_files(receive_from_data).await {
                // NOTE: we don't want to mess the process if an error comes in, keep doing it
                println!("[integrations][receive] error: {e}");
            }
            println!("[integrations][receive] end");
        }

        // handle the sends
        let send_to_data: Vec<integrations::SendToKind> = send_to
            .into_iter()
            .filter_map(|(target_id, data)| {
                if let Some(target) = cnf.targets.iter().find(|t| t.id == target_id) {
                    if !target.enable
                        || (target.mode != config::TargetMode::Push
                            && target.mode != config::TargetMode::PushPull)
                    {
                        return None;
                    }

                    return Some(data);
                }

                None
            })
            .collect();

        if !send_to_data.is_empty() {
            println!("[integrations][send] start...");
            if let Err(e) = integrations_mod.send_files(send_to_data).await {
                // NOTE: we don't want to mess the process if an error comes in, keep doing it
                println!("[integrations][send] error: {e}");
            }
            println!("[integrations][send] end");
        }

        // wait for the next loop iteration
        sleep(Duration::from_millis(cnf.loop_sleep_time_ms)).await;
    }

    // close the integrations
    integrations_mod.close().await.unwrap();
}

fn get_target_file_debounce_ms(target: &config::Target) -> u64 {
    let mut debounce_ms: u64 = 1000;
    if let Some(debounce) = target.change_debounce_sec {
        debounce_ms = debounce * 1000;
    }

    debounce_ms
}

fn calc_target_unlock_file_debounce(cnf: &config::Config, target: &config::Target) -> u64 {
    // the file watcher has a debounce for file change
    // the locking mechanism exists to prevent that change event
    // to trigger and loop. as such, we want to make sure we unlock
    // only after the debounce is done
    // we use *2 because there are 2 loops that can change this
    // so... 1 loop time per the 2 loops + the target debounce
    cnf.loop_sleep_time_ms * 2 + get_target_file_debounce_ms(target)
}

fn changed_target_to_integration_kind(
    target: &config::Target,
    changed_target: path_watcher::ChangedTarget,
) -> Result<Option<(String, integrations::SendToKind)>> {
    match target.kind {
        // handle the local
        config::TargetKind::Local => match &target.data_dest {
            Some(dest) => {
                let base = changed_target.src.replace(&target.src, "");
                let relative_path = if let Some(base) = base.strip_prefix("/") {
                    base.to_owned()
                } else {
                    base
                };

                let mod_target = integrations::SendToKind::Local(integrations::local::SendToData {
                    id: target.id.to_owned(),
                    src_full: changed_target.src,
                    src_relative: relative_path,
                    dest: dest.clone(),
                    timestamp: changed_target.timestamp,
                });

                return Ok(Some((target.id.to_owned(), mod_target)));
            }
            _ => {
                return Err(anyhow!(format!(
                    "target \"{}\" does not have the required parameters",
                    &target.id
                )));
            }
        },

        // handle the p2p
        config::TargetKind::P2p => {
            let node_id = target.data_node.clone().unwrap_or("".to_owned());
            let dest_id = target.data_dest.clone().unwrap_or("".to_owned());
            if node_id.is_empty() || dest_id.is_empty() {
                return Err(anyhow!(format!(
                    "target \"{}\" does not have the required parameters",
                    &target.id
                )));
            }

            let base = changed_target.src.replace(&target.src, "");
            let relative_path = if let Some(base) = base.strip_prefix("/") {
                base.to_owned()
            } else {
                base
            };
            let mod_target = integrations::SendToKind::P2p(integrations::p2p::SendToData {
                id: target.id.to_owned(),
                src_full: changed_target.src,
                src_relative: relative_path,
                node_id,
                dest_id,
                timestamp: changed_target.timestamp,
            });

            return Ok(Some((target.id.to_owned(), mod_target)));
        }
        _ => {
            println!("module not implemented: {}", target.kind);
        }
    }

    Ok(None)
}
