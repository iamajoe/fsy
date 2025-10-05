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
