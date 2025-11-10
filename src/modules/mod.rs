mod local;

use anyhow::Result;

pub enum TargetKind {
    // src_full, src_relative, dest
    Local(String, String, String),
}

pub struct TargetKindModules {
    // TODO: set p2p connection as option
}

impl TargetKindModules {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn send_target(&self, kind: TargetKind) -> Result<()> {
        match kind {
            TargetKind::Local(src_full, src_relative, dest) => local::send_file(&src_full, &src_relative, &dest),
        }
    }

    pub fn close(&self) -> Result<()> {
        // TODO: close p2p connection
        Ok(())
    }
}
