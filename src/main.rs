mod config;
mod file_ledger_repository;
mod integrations;
mod path_watcher;

use std::time::Duration;

use anyhow::{Result, anyhow};
use chrono::Utc;
use tokio::sync::{mpsc, watch};
use tokio::time::sleep;

use self::path_watcher::PathWatcher;

const CHANNEL_BUFFER_SIZE: usize = 1000;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut config_dir_path = "".to_owned();
    if args.len() >= 2 {
        config_dir_path = args[1].clone();
    }
    println!("running config dir: {config_dir_path}");

    let config = config::Config::new(&config_dir_path).unwrap();
    let file_repo = file_ledger_repository::FileLedgerRepository::new(
        config.db_path.clone().into_string().unwrap(),
    );
    file_repo.migrate().unwrap();

    // NOTE: controller if the app is running or not
    let (is_running_tx, is_running_rx) = watch::channel(true);
    let (changed_target_data_tx, mut changed_target_data_rx) = mpsc::channel(CHANNEL_BUFFER_SIZE);

    // TODO: need to handle the crons

    // loop watcher
    let target_watcher_is_running_rx = is_running_rx.clone();
    let target_watcher_repo = file_repo.clone();
    let target_watcher_config = config.clone();
    tokio::spawn(async move {
        println!("[target_watcher] starting");
        // create the watchers
        let watchers: Vec<(String, PathWatcher)> = target_watcher_config
            .targets
            .iter()
            .filter_map(|t| {
                if !t.enable
                    || (t.mode != config::TargetMode::Push
                        && t.mode != config::TargetMode::PushPull)
                {
                    return None;
                }

                let debounce_ms = get_target_file_debounce_ms(t);
                let path_watcher = PathWatcher::new(vec![t.src.clone()], debounce_ms).unwrap();

                Some((t.id.clone(), path_watcher))
            })
            .collect();

        // loop through the possible changes incoming
        if watchers.is_empty() {
            return;
        }

        println!("[target_watcher] looping");
        loop {
            if !*target_watcher_is_running_rx.borrow() {
                break;
            }

            // check all watcher to targets
            let changed_target_data: Vec<(String, path_watcher::ChangedTarget)> = watchers
                .iter()
                .filter_map(|(target_id, watcher)| {
                    // check for changed targets
                    if let Ok(Some(changed_target)) = watcher.get_changed_target() {
                        // file is locked so any changes should be disregarded
                        let is_locked = target_watcher_repo
                            .is_file_locked(&changed_target.full_path)
                            .unwrap();
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
                    println!("Something went wrong sending changed target to channel: {e}");
                }
            }

            sleep(Duration::from_millis(
                target_watcher_config.loop_sleep_time_ms,
            ))
            .await;
        }

        // close all the watchers
        for (_, mut watcher) in watchers {
            watcher.close().unwrap();
        }
    });

    // loop target kind handler
    let integrations_is_running_rx = is_running_rx.clone();
    let integrations_file_repo = file_repo.clone();
    let integrations_config = config.clone();
    tokio::spawn(async move {
        println!("[target] looping");

        // NOTE: only enable p2p if there a target with that kind
        let has_p2p = integrations_config
            .targets
            .iter()
            .any(|t| t.enable && t.kind == config::TargetKind::P2p);

        let mut integrations_mod = integrations::Integrations::new(
            integrations_file_repo,
            has_p2p,
            &integrations_config.p2p_secret_key,
        )
        .await
        .unwrap();

        loop {
            if !*integrations_is_running_rx.borrow() {
                break;
            }

            let mut send_to: Vec<(String, integrations::IntegrationToKind)> = vec![];
            let mut receive_from: Vec<(String, integrations::IntegrationFromKind)> = vec![];

            // handle the targets incoming from other threads
            while let Ok((target_id, changed_target)) = changed_target_data_rx.try_recv() {
                if let Some(target) = integrations_config
                    .targets
                    .iter()
                    .find(|t| t.id == target_id)
                {
                    match changed_target_to_integration_kind(target, changed_target) {
                        Ok(Some(data)) => {
                            send_to.push(data);
                        }
                        Err(e) => {
                            // NOTE: we don't want to mess the process if an error comes in, keep doing it
                            println!("[target][change] error: {e}");
                        }
                        _ => {}
                    }
                }
            }

            // handle integration events
            match integrations_mod.check_events().await {
                Ok((mut send_to_arr, mut receive_from_arr)) => {
                    send_to.append(&mut send_to_arr);
                    receive_from.append(&mut receive_from_arr);
                }
                Err(e) => {
                    // NOTE: we don't want to mess the process if an error comes in, keep doing it
                    println!("[target][check_events] error: {e}");
                }
            }

            // handle the sends
            for (_target_id, send_to_data) in send_to {
                println!("[target][send] start...");

                let start = Utc::now().timestamp_millis();
                if let Err(e) = integrations_mod.send_file(send_to_data).await {
                    // NOTE: we don't want to mess the process if an error comes in, keep doing it
                    println!("[target][send] error: {e}");
                }

                let time_spent = Utc::now().timestamp_millis() - start;
                println!("[target][send] end ({time_spent}ms)");
            }

            // handle the receivals
            for (target_id, receive_from_data) in receive_from {
                if let Some(target) = integrations_config
                    .targets
                    .iter()
                    .find(|t| t.id == target_id)
                {
                    println!("[target][receive] start...");
                    let debounce = calc_target_unlock_file_debounce(&integrations_config, target);

                    // TODO: need to retrieve the wait unlock!
                    let start = Utc::now().timestamp_millis();

                    if let Err(e) = integrations_mod
                        .receive_file(receive_from_data, debounce)
                        .await
                    {
                        // NOTE: we don't want to mess the process if an error comes in, keep doing it
                        println!("[target][receive] error: {e}");
                    }

                    let time_spent = Utc::now().timestamp_millis() - start;
                    println!("[target][receive] end ({time_spent}ms)");
                }
            }

            // wait for the next loop iteration
            sleep(Duration::from_millis(
                integrations_config.loop_sleep_time_ms,
            ))
            .await;
        }

        // close the integrations
        integrations_mod.close().await.unwrap();
    });

    // wait for all the keyboard events
    // included will be the signal exit
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for event");
    println!("closing");

    // shut the threads
    is_running_tx.send(false).unwrap();

    Ok(())
}

fn changed_target_to_integration_kind(
    target: &config::Target,
    changed_target: path_watcher::ChangedTarget,
) -> Result<Option<(String, integrations::IntegrationToKind)>> {
    match target.kind {
        // handle the local
        config::TargetKind::Local => match &target.data_dest {
            Some(dest) => {
                // id, src_full, src_relative, dest, timestamp
                let mod_target =
                    integrations::IntegrationToKind::Local(integrations::local::SendToData {
                        id: target.id.to_owned(),
                        src_full: changed_target.full_path,
                        src_relative: changed_target.relative_path,
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
            println!("[watcher][changed_target][p2p] sending target");
            // TODO: these unwraps should be logged and that is it
            let node_id = target.data_node_id.to_owned().unwrap();
            let dest_id = target.data_dest_id.to_owned().unwrap();

            let mod_target = integrations::IntegrationToKind::P2p(integrations::p2p::SendToData {
                id: target.id.to_owned(),
                src_full: changed_target.full_path,
                src_relative: changed_target.relative_path,
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
