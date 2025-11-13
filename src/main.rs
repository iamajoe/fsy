mod config;
mod modules;
mod path_watcher;
mod repository;

use std::time::Duration;

use anyhow::{Result, bail};
use chrono::Utc;
use tokio::sync::{mpsc, watch};
use tokio::time::sleep;

use self::config::TargetMode;
use self::path_watcher::PathWatcher;

const CHANNEL_BUFFER_SIZE: usize = 1000;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut config_dir_path = "".to_owned();
    if args.len() >= 2 {
        config_dir_path = args[1].clone();
    }

    let config = config::Config::new(&config_dir_path).unwrap();

    // NOTE: controller if the app is running or not
    let (is_running_tx, is_running_rx) = watch::channel(true);
    let (target_kinds_tx, mut target_kinds_rx) = mpsc::channel(CHANNEL_BUFFER_SIZE);

    // TODO: need to handle the crons

    // loop watcher
    let watcher_is_running_rx = is_running_rx.clone();
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

                let mut target_kinds: Vec<modules::TargetKind> = vec![];

                // check all watcher targets
                for (target_id, watcher) in &watchers {
                    // check for changed targets
                    if let Ok(Some(changed_target)) = watcher.get_changed_target() {
                        let target = config.targets.iter().find(|t| *t.id == *target_id).unwrap();
                        match target.kind {
                            // handle the local
                            config::TargetKind::Local => match &target.data_dest {
                                Some(dest) => {
                                    println!("[watcher][changed_target][local] sending target");
                                    target_kinds.push(modules::TargetKind::Local(
                                        changed_target.full_path,
                                        changed_target.relative_path,
                                        dest.clone(),
                                        changed_target.timestamp,
                                    ));
                                }
                                _ => {
                                    bail!("target \"{}\" does not have the required parameters", &target.id)
                                }
                            },
                            _ => {
                                println!("module not implemented: {}", target.kind);
                            }
                        }
                    }
                }

                // cache the target kind to be handled
                for target_kind in target_kinds {
                    if let Err(e) = target_kinds_tx.send(target_kind).await {
                        println!("Something went wrong sending target kind to channel: {e}");
                    }
                }

                // TODO: could be on a config at least
                sleep(Duration::from_millis(1000)).await;
            }
        }

        // close all the watchers
        for (_, mut watcher) in watchers {
            watcher.close().unwrap();
        }

        Ok(())
    });

    // loop target kind handler
    let target_is_running_rx = is_running_rx.clone();
    tokio::spawn(async move {
        println!("[target] looping");

        let kind_modules = modules::TargetKindModules::new();

        loop {
            if !*target_is_running_rx.borrow() {
                break;
            }

            // handle the targets incoming from other threads
            while let Ok(target_kind) = target_kinds_rx.try_recv() {
                println!("[target][send] processing and sending");
                let start = Utc::now().timestamp_millis();

                if let Err(e) = kind_modules.send_target(target_kind).await {
                    // NOTE: we don't want to mess the process if an error comes in, keep doing it
                    println!("[target][send] error: {e}");
                }

                let time_spent = Utc::now().timestamp_millis() - start;
                println!("[target][send] end ({time_spent}ms)");
            }
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
