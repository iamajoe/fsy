use anyhow::Result;
use std::path::Path;

use crate::{target, action, connection, path_watcher};

// run_event_check is run when there is an event on the connection
// or the sync process. For example:
// - a received message through the connection
//   - it parses then the message to be of the type of action
// - targets have changed on the syncing process
//   - it creates then actions to send through the connection
pub async fn run(
    nodes: &[target::NodeData],
    target_groups: &[target::TargetGroup],
    conn_event: Option<connection::ConnEvent>,
    changed_targets: Option<Vec<path_watcher::ChangedTarget>>,
) -> Result<Vec<action::CommAction>> {
    let mut actions: Vec<action::CommAction> = vec![];

    // check for events on the connection
    if let Some(connection::ConnEvent::ReceivedMessage(node_id, raw_msg)) = conn_event {
        println!("[event_check][conn] message received: {node_id}");
        let action = action::CommAction::from_namespaced_msg(&node_id, &raw_msg);
        actions.push(action);
    }

    // check if watcher has changed targets events
    if let Some(targets) = changed_targets {
        println!("[event_check][watcher] targets changed: {}", targets.len());

        // retrieve nodes of the affected target groups and map to the action
        for changed_target in targets {
            // check if we have a lock in place, if we have, there is an update going,
            // we don't want to create a change upon that
            let file_path = Path::new(&changed_target.base_path).join(&changed_target.relative_path);
            let file_path = action::get_target_locked_path(file_path);
            if action::is_target_locked(&file_path) {
                continue;
            }

            let groups =
                target::get_push_groups_with_path(target_groups, &changed_target.base_path);
            for group in groups {
                let target_actions: Vec<action::CommAction> = group
                    .get_node_ids(
                        nodes,
                        &[target::TargetMode::Push, target::TargetMode::PushPull],
                    )
                    .iter()
                    .map(|node_id| {
                        action::CommAction::TargetHasChanged(
                            node_id.to_owned(),
                            group.name.clone(),
                            changed_target.relative_path.clone(),
                        )
                        .to_send_message()
                    })
                    .collect();
                actions.extend(target_actions);
            }
        }
    }

    Ok(actions)
}

