use chrono::{DateTime, Utc};

use crate::config::{FileSync, NodeData};

const FILE_HAS_CHANGED_NAMESPACE: u8 = 0;

#[derive(Debug, Clone)]
pub enum CommAction {
    // -------------------------------------
    // RAW for the connection

    // SendMessage used to send messages through the connection to a node
    // - SendMessage(node_id, msg)
    SendMessage(String, String),

    // -------------------------------------
    // Actual actions

    // FileHasChanged used for when the pusher wants to inform that a file has changed
    // - FileHasChanged(node_id, file_path, change_date)
    FileHasChanged(String, String, DateTime<Utc>),

    // RequestFile used for the puller to request for a specific file
    // - RequestFile(node_id, file_path)
    RequestFile(String, String),
}

pub fn get_changed_files_actions(
    file_targets: Vec<FileSync>,
    nodes: Vec<NodeData>,
) -> Vec<CommAction> {
    if file_targets.is_empty() {
        return vec![];
    }

    let namespace = FILE_HAS_CHANGED_NAMESPACE;
    let now = Utc::now().timestamp();

    file_targets
        .iter()
        .flat_map(|file_target| {
            let changed_path = file_target.path.clone();
            let mut actions: Vec<CommAction> = vec![];

            for target in file_target.targets.iter() {
                let node = nodes.iter().find(|n| n.name == target.trustee_name);
                if let Some(node) = node {
                    // TODO: what about the key?! we use the key to decrypt that depending on
                    let msg = format!("[{namespace}]->{now};{}", &changed_path).to_string();
                    actions.push(CommAction::SendMessage(node.node_id.clone(), msg));
                }
            }

            actions
        })
        .collect()
}

pub fn parse_msg_to_action(node_id: String, raw_msg: String) -> Option<CommAction> {
    // check if we have a module
    let mod_sep = raw_msg.split_once("]->");
    if let Some(raw_msg) = mod_sep {
        let module = raw_msg.0;
        let raw_msg = raw_msg.1;

        // check for file has changed
        let namespace = FILE_HAS_CHANGED_NAMESPACE;
        let namespace_str = format!("[{namespace}");
        if module.contains(&namespace_str) {
            return parse_file_has_changed_to_action(node_id, raw_msg);
        }
    }

    None
    // TODO: need to handle request file as well
}

fn parse_file_has_changed_to_action(node_id: String, raw_msg: &str) -> Option<CommAction> {
    let res: Vec<&str> = raw_msg.split(";").collect();
    if res.len() != 2 {
        return None;
    }

    let timestamp = res[1].parse::<i64>();
    if let Ok(timestamp) = timestamp {
        let timestamp = DateTime::from_timestamp(timestamp, 0);
        if let Some(timestamp) = timestamp {
            return Some(CommAction::FileHasChanged(
                node_id,
                res[0].to_owned(),
                timestamp,
            ));
        }
    }

    None
}
