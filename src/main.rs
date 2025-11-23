use anyhow::Result;
use fsy::core::{config, file_ledger_repository};
use tokio::sync::{mpsc, watch};

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
    let (changed_target_data_tx, changed_target_data_rx) = mpsc::channel(CHANNEL_BUFFER_SIZE);

    // run start process
    fsy::run_start_process(&config, &file_repo, changed_target_data_tx.clone()).unwrap();

    // run the cron process
    let cron_is_running_rx = is_running_rx.clone();
    let cron_repo = file_repo.clone();
    let cron_config = config.clone();
    let cron_changed_target_tx = changed_target_data_tx.clone();
    tokio::spawn(async move {
        println!("[cron] starting");
        fsy::run_cron_process(
            cron_is_running_rx,
            &cron_config,
            &cron_repo,
            cron_changed_target_tx,
        );
    });

    // loop target watcher
    let target_watcher_is_running_rx = is_running_rx.clone();
    let target_watcher_repo = file_repo.clone();
    let target_watcher_config = config.clone();
    let target_watcher_changed_target_tx = changed_target_data_tx.clone();
    tokio::spawn(async move {
        println!("[target_watcher] starting");
        fsy::run_watch_process(
            target_watcher_is_running_rx,
            &target_watcher_config,
            &target_watcher_repo,
            target_watcher_changed_target_tx,
        )
        .await;
    });

    // loop integrations
    let integrations_is_running_rx = is_running_rx.clone();
    let integrations_file_repo = file_repo.clone();
    let integrations_config = config.clone();
    tokio::spawn(async move {
        println!("[integrations] starting");
        fsy::run_integrations_process(
            integrations_is_running_rx,
            &integrations_config,
            &integrations_file_repo,
            changed_target_data_rx,
        )
        .await;
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
