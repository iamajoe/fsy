use crate::*; // TODO: we should probably not import the whole thing

use notify::Watcher;
use std::{fs, path::Path};
use tokio::sync::watch::Receiver;

pub fn watch_path(
    p: &Path,
    mut cb: impl FnMut(Result<notify::Event>),
    is_running_rx: &Receiver<bool>,
) -> Result<()> {
    // no point in watching what doesn't exist
    if !fs::exists(p).unwrap() {
        return Ok(());
    }

    // figure which recursive mode should be used
    let meta = fs::metadata(p).unwrap();
    let recurse = if meta.is_dir() {
        notify::RecursiveMode::Recursive
    } else {
        notify::RecursiveMode::NonRecursive
    };

    println!("watching path: {:?}", &p);

    // setup the watcher
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(tx).unwrap();
    watcher.watch(p, recurse).unwrap();

    // block forever, printing out events as they come in
    for res in rx {
        // check if still running or if already all canceled
        let is_running = *is_running_rx.borrow();
        if !is_running {
            println!("breaking the watcher chain...");
            watcher.unwatch(p).unwrap();
            break;
        }

        println!("inside events...");

        match res {
            Ok(event) => cb(Ok(event)),
            Err(e) => {
                println!("Something went wrong with watcher: {e}");
                cb(Err(Error::Unknown));
            }
        }
    }

    Ok(())
}
