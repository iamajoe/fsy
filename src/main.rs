mod config;
mod file_ledger_repository;
mod modules;
mod path_watcher;

use std::time::Duration;

use anyhow::{Result, anyhow};
use chrono::Utc;
use tokio::sync::{mpsc, watch};
use tokio::time::sleep;

use self::config::{Target, TargetMode};
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
    let repo =
        file_ledger_repository::FileLedgerRepository::new(config.db_path.into_string().unwrap());
    repo.migrate().unwrap();

    // NOTE: controller if the app is running or not
    let (is_running_tx, is_running_rx) = watch::channel(true);
    let (target_kinds_tx, mut target_kinds_rx) = mpsc::channel(CHANNEL_BUFFER_SIZE);

    // TODO: need to handle the crons

    // loop watcher
    let watcher_is_running_rx = is_running_rx.clone();
    let watcher_repo = repo.clone();
    tokio::spawn(async move {
        println!("[watcher] starting");
        // create the watchers
        let watchers: Vec<(String, PathWatcher)> = config
            .targets
            .iter()
            .filter_map(|t| {
                if !t.enable || (t.mode != TargetMode::Push && t.mode != TargetMode::PushPull) {
                    return None;
                }

                let mut debounce_ms: u64 = 1000;
                if let Some(debounce) = t.change_debounce_sec {
                    debounce_ms = debounce * 1000;
                }

                let path_watcher = PathWatcher::new(vec![t.src.clone()], debounce_ms).unwrap();

                Some((t.id.clone(), path_watcher))
            })
            .collect();

        // loop through the possible changes incoming
        if !watchers.is_empty() {
            println!("[watcher] looping");
            loop {
                if !*watcher_is_running_rx.borrow() {
                    break;
                }

                // check all watcher targets
                let target_kinds: Vec<(modules::TargetKind, Option<u64>)> = watchers
                    .iter()
                    .filter_map(|(target_id, watcher)| {
                        get_watcher_target_kind(watcher, &watcher_repo, &config.targets, target_id)
                            .unwrap()
                    })
                    .collect();

                // cache the target kind to be handled
                for target_kind in target_kinds {
                    if let Err(e) = target_kinds_tx.send(target_kind).await {
                        println!("Something went wrong sending target kind to channel: {e}");
                    }
                }

                sleep(Duration::from_millis(config.loop_sleep_time_ms)).await;
            }
        }

        // close all the watchers
        for (_, mut watcher) in watchers {
            watcher.close().unwrap();
        }
    });

    // loop target kind handler
    let target_is_running_rx = is_running_rx.clone();
    let target_repo = repo.clone();
    tokio::spawn(async move {
        println!("[target] looping");

        let kind_modules = modules::TargetKindModules::new(target_repo);

        loop {
            if !*target_is_running_rx.borrow() {
                break;
            }

            // handle the targets incoming from other threads
            while let Ok((target_kind, change_debounce_sec)) = target_kinds_rx.try_recv() {
                println!("[target][send] processing and sending");
                let start = Utc::now().timestamp_millis();

                // the file watcher has a debounce for file change
                // the locking mechanism exists to prevent that change event
                // to trigger and loop. as such, we want to make sure we unlock
                // only after the debounce is done
                // we use *2 because there are 2 loops that can change this
                // so... 1 loop time per the 2 loops + the target debounce
                let mut wait_unlock_millis = config.loop_sleep_time_ms * 2;
                if let Some(change_debounce_sec) = change_debounce_sec {
                    wait_unlock_millis += change_debounce_sec * 1000;
                }

                if let Err(e) = kind_modules
                    .send_target(target_kind, wait_unlock_millis)
                    .await
                {
                    // NOTE: we don't want to mess the process if an error comes in, keep doing it
                    println!("[target][send] error: {e}");
                }

                let time_spent = Utc::now().timestamp_millis() - start;
                println!("[target][send] end ({time_spent}ms)");
            }

            sleep(Duration::from_millis(config.loop_sleep_time_ms)).await;
        }

        // close the modules
        kind_modules.close().unwrap();
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

fn get_watcher_target_kind(
    watcher: &PathWatcher,
    watcher_repo: &file_ledger_repository::FileLedgerRepository,
    targets: &[Target],
    target_id: &str,
) -> Result<Option<(modules::TargetKind, Option<u64>)>> {
    // check for changed targets
    if let Ok(Some(changed_target)) = watcher.get_changed_target() {
        // file is locked so any changes should be disregarded
        let is_locked = watcher_repo
            .is_file_locked(&changed_target.full_path)
            .unwrap();
        if is_locked {
            return Ok(None);
        }

        let target = targets.iter().find(|t| *t.id == *target_id).unwrap();

        match target.kind {
            // handle the local
            config::TargetKind::Local => match &target.data_dest {
                Some(dest) => {
                    println!("[watcher][changed_target][local] sending target");
                    let mod_target = modules::TargetKind::Local(
                        target_id.to_owned(),
                        changed_target.full_path,
                        changed_target.relative_path,
                        dest.clone(),
                        changed_target.timestamp,
                    );

                    return Ok(Some((mod_target, target.change_debounce_sec)));
                }
                _ => {
                    return Err(anyhow!(format!(
                        "target \"{}\" does not have the required parameters",
                        &target.id
                    )));
                }
            },
            _ => {
                println!("module not implemented: {}", target.kind);
            }
        }
    }

    Ok(None)
}
