use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NodeData {
    pub name: String, // unique identifier of this node for the user
    pub id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum TargetMode {
    #[serde(rename = "push")]
    Push,
    #[serde(rename = "push-pull")]
    PushPull,
    #[serde(rename = "pull")]
    Pull,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Target {
    pub mode: TargetMode,  // is it only push? only pull? both?
    pub node_name: String, // trustee name, the descritive
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TargetGroup {
    pub name: String, // name identifier to be passed as unique communicator between nodes
    pub path: String, // path for the file / folder
    pub targets: Vec<Target>, // targets to whom push / pull
}

impl TargetGroup {
    pub fn get_node_ids(&self, nodes: &[NodeData], modes: &[TargetMode]) -> Vec<String> {
        let target_names: Vec<String> = self
            .targets
            .iter()
            .filter_map(|t| {
                if !modes.contains(&t.mode) {
                    return None;
                }

                Some(t.node_name.clone())
            })
            .collect();

        nodes
            .iter()
            .filter_map(|node| {
                if !target_names.contains(&node.name) {
                    return None;
                }

                Some(node.id.clone())
            })
            .collect()
    }
}

pub fn get_push_group_with_name(groups: &[TargetGroup], name: &str) -> Option<TargetGroup> {
    groups
        .iter()
        .find_map(|item| {
            let found = item
                .targets
                .iter()
                .any(|t| t.mode == TargetMode::Push || t.mode == TargetMode::PushPull);
            if !found || item.name != name {
                return None;
            }

            Some(item.clone())
        })
}

pub fn get_push_group_paths(groups: &[TargetGroup]) -> Vec<String> {
    groups
        .iter()
        .filter_map(|item| {
            let found = item
                .targets
                .iter()
                .any(|t| t.mode == TargetMode::Push || t.mode == TargetMode::PushPull);
            if !found {
                return None;
            }

            Some(item.path.clone())
        })
        .collect()
}

pub fn get_push_groups_with_path(groups: &[TargetGroup], file_path: &str) -> Vec<TargetGroup> {
    groups
        .iter()
        .filter_map(|item| {
            let found = item
                .targets
                .iter()
                .any(|t| t.mode == TargetMode::Push || t.mode == TargetMode::PushPull);
            if !found {
                return None;
            }

            if item.path != file_path {
                return None;
            }

            Some(item.clone())
        })
        .collect()
}

pub fn get_pull_group_paths(groups: &[TargetGroup]) -> Vec<String> {
    groups
        .iter()
        .filter_map(|item| {
            let found = item
                .targets
                .iter()
                .any(|t| t.mode == TargetMode::Pull || t.mode == TargetMode::PushPull);
            if !found {
                return None;
            }

            Some(item.path.clone())
        })
        .collect()
}

pub fn get_pull_group_with_name(groups: &[TargetGroup], name: &str) -> Option<TargetGroup> {
    groups
        .iter()
        .find_map(|item| {
            let found = item
                .targets
                .iter()
                .any(|t| t.mode == TargetMode::Pull || t.mode == TargetMode::PushPull);
            if !found || item.name != name {
                return None;
            }

            Some(item.clone())
        })
}

pub fn group_has_node_id(group: &TargetGroup, nodes: &[NodeData], node_id: &str) -> bool {
    nodes.iter().any(|node| {
        if node.id != node_id {
            return false;
        }

        group.targets.iter().any(|target| target.node_name == node.name)
    })
}
