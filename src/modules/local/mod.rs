use std::fs;

use anyhow::{Result, anyhow};

pub fn receive_file(src_full: &str, dest: &str) -> Result<()> {
    if let Err(e) = fs::copy(src_full, dest) {
        return Err(anyhow!(e));
    }

    Ok(())
}
