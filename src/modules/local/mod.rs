use std::fs;
use std::path::Path;

use anyhow::{Result, anyhow};

pub fn send_file(src_full: &str, src_relative: &str, dest: &str) -> Result<()> {
    let mut dest_full = dest.to_owned();

    // empty relative means it is a file, not a directory
    // as a directory, we need to add the relative
    if !src_relative.is_empty() {
        let dest_full_raw = Path::new(dest).join(src_relative);
        dest_full = dest_full_raw.to_str().unwrap().to_owned();
    }

    if let Err(e) = fs::copy(src_full, dest_full) {
        return Err(anyhow!(e));
    }

    Ok(())
}
