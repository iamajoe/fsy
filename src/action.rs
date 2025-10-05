use anyhow::Result;
use chrono::{DateTime, Utc};
use std::fmt;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::connection::Connection;
use crate::queue;
use crate::target_watcher::SyncProcess;

enum ActionNamespace {
    Unknown,
    SendMessage,
    TargetHasChanged,
    RequestTarget,
    DownloadTarget,
    DownloadDone,
    // TODO: maybe we can use the blob checksum instead of timestamp?!
    //       timestamp is flawed in various ways
    RequestTargetTimestamp,
    TargetTimestamp,
}

impl ActionNamespace {
    fn to_u8(&self) -> u8 {
        match self {
            ActionNamespace::SendMessage => 1,
            ActionNamespace::TargetHasChanged => 2,
            ActionNamespace::RequestTarget => 3,
            ActionNamespace::DownloadTarget => 4,
            ActionNamespace::DownloadDone => 5,
            ActionNamespace::RequestTargetTimestamp => 6,
            ActionNamespace::TargetTimestamp => 7,
            _ => 0,
        }
    }
}

impl From<String> for ActionNamespace {
    fn from(value: String) -> Self {
        let value = value.as_bytes().to_owned();
        if value.is_empty() {
            return ActionNamespace::Unknown;
        }
        let value = value[0];

        match value {
            1 => ActionNamespace::SendMessage,
            2 => ActionNamespace::TargetHasChanged,
            3 => ActionNamespace::RequestTarget,
            4 => ActionNamespace::DownloadTarget,
            5 => ActionNamespace::DownloadDone,
            6 => ActionNamespace::RequestTargetTimestamp,
            7 => ActionNamespace::TargetTimestamp,
            _ => ActionNamespace::Unknown,
        }
    }
}

impl fmt::Display for ActionNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let raw = self.to_u8();
        write!(f, "{raw}")
    }
}

fn get_ns_split(raw_msg: &str) -> (ActionNamespace, String) {
    if let Some(raw_msg) = raw_msg.split_once("]]::") {
        let module = raw_msg.0.to_owned();
        return (ActionNamespace::from(module), raw_msg.1.to_owned());
    }

    (ActionNamespace::Unknown, "".to_owned())
}

fn template_msg_with_ns(namespace: ActionNamespace, raw_msg: &str) -> String {
    format!("{namespace}]]::{raw_msg}")
}

#[derive(Debug, Clone)]
pub enum CommAction {
    Unknown,

    // SendMessage: send messages through the connection to a node
    // - SendMessage(to_node_id, msg)
    SendMessage(String, String),

    // TargetHasChanged: pusher inform that target has changed to puller node
    // - TargetHasChanged(to_node_id, target_name)
    TargetHasChanged(String, String),

    // RequestTarget: puller requests target from pusher node
    // - RequestTarget(from_node_id, target_name)
    RequestTarget(String, String),

    // DownloadTarget: puller takes ticket_id and downloads it
    // - DownloadTarget(from_node_id, ticket_id)
    DownloadTarget(String, String),

    // DownloadDone: pusher knows download is done and closes the ticket
    // - DownloadDone(from_node_id, ticket_id)
    DownloadDone(String, String),

    // RequestTargetTimestamp: puller wants to know the timestamp of target
    // - RequestTargetTimestamp(from_node_id, target_name)
    RequestTargetTimestamp(String, String),

    // TargetTimestamp: pushed informs the timestamp of a target
    // - TargetTimestamp(from_node_id, target_name, last_update_timestamp)
    TargetTimestamp(String, String, DateTime<Utc>),
}

impl CommAction {
    pub fn from_namespaced_msg(node_id: &str, raw_msg: &str) -> Self {
        let (module, raw_msg) = get_ns_split(raw_msg);
        match module {
            ActionNamespace::SendMessage => {
                Self::SendMessage(node_id.to_owned(), raw_msg.to_owned())
            }
            ActionNamespace::TargetHasChanged => {
                Self::TargetHasChanged(node_id.to_owned(), raw_msg.to_owned())
            }
            ActionNamespace::RequestTarget => {
                Self::RequestTarget(node_id.to_owned(), raw_msg.to_owned())
            }
            ActionNamespace::DownloadTarget => {
                Self::DownloadTarget(node_id.to_owned(), raw_msg.to_owned())
            }
            ActionNamespace::DownloadDone => {
                Self::DownloadDone(node_id.to_owned(), raw_msg.to_owned())
            }
            ActionNamespace::RequestTargetTimestamp => {
                Self::RequestTargetTimestamp(node_id.to_owned(), raw_msg.to_owned())
            }
            ActionNamespace::TargetTimestamp => {
                if let Some(raw_msg) = raw_msg.split_once(";") {
                    let timestamp = raw_msg.1.parse::<i64>();
                    if let Ok(timestamp) = timestamp {
                        let timestamp = DateTime::from_timestamp(timestamp, 0);
                        if let Some(timestamp) = timestamp {
                            return Self::TargetTimestamp(
                                node_id.to_owned(),
                                raw_msg.0.to_string(),
                                timestamp,
                            );
                        }
                    }
                }

                Self::Unknown
            }
            _ => Self::Unknown,
        }
    }

    pub fn to_send_message(&self) -> Self {
        match self {
            Self::SendMessage(_to_node_id, _msg) => self.clone(),
            Self::TargetHasChanged(to_node_id, target_name) => {
                let msg = template_msg_with_ns(ActionNamespace::TargetHasChanged, target_name);
                Self::SendMessage(to_node_id.to_owned(), msg)
            }
            Self::RequestTarget(to_node_id, target_name) => {
                let msg = template_msg_with_ns(ActionNamespace::RequestTarget, target_name);
                Self::SendMessage(to_node_id.to_owned(), msg)
            }
            Self::DownloadTarget(from_node_id, ticket_id) => {
                let msg = template_msg_with_ns(ActionNamespace::DownloadTarget, ticket_id);
                Self::SendMessage(from_node_id.to_owned(), msg)
            }
            Self::DownloadDone(from_node_id, ticket_id) => {
                let msg = template_msg_with_ns(ActionNamespace::DownloadDone, ticket_id);
                Self::SendMessage(from_node_id.to_owned(), msg)
            }
            Self::RequestTargetTimestamp(from_node_id, target_name) => {
                let msg = template_msg_with_ns(ActionNamespace::RequestTargetTimestamp, target_name);
                Self::SendMessage(from_node_id.to_owned(), msg)
            }
            Self::TargetTimestamp(from_node_id, target_name, timestamp) => {
                let msg = format!("{target_name};{timestamp}");
                let msg = template_msg_with_ns(ActionNamespace::TargetTimestamp, &msg);
                Self::SendMessage(from_node_id.to_owned(), msg)
            }

            // do nothing on extra not handled stuff
            _ => Self::Unknown,
        }
    }
}

pub async fn perform_action(
    conn: &Arc<Mutex<Connection>>,
    sync_process: &SyncProcess,
    actions_queue: &Arc<Mutex<queue::Queue<CommAction>>>,
    action: Option<CommAction>,
) -> Result<()> {
    match action {
        // we have a new message to send through the connection
        Some(CommAction::SendMessage(to_node_id, msg)) => {
            println!("action: SendMessage: {to_node_id}, {msg}");
            return on_send_message(conn, to_node_id, msg).await;
        }

        // received a target changed, lets then request the target if that is the case
        Some(CommAction::TargetHasChanged(to_node_id, target_name)) => {
            println!("action: TargetHasChanged: {to_node_id}, {target_name}");
            return on_target_has_changed(sync_process, actions_queue, to_node_id, target_name)
                .await;
        }

        // a request has been done by the puller, as such we prepare the ticket id
        // and send the message to the puller
        Some(CommAction::RequestTarget(from_node_id, target_name)) => {
            println!("action: RequestTarget: {from_node_id}, {target_name}");
            return on_request_target(from_node_id, target_name).await;
        }

        // pusher has prepared a ticket id for us to download if we want
        Some(CommAction::DownloadTarget(from_node_id, ticket_id)) => {
            println!("action: DownloadTarget: {from_node_id}, {ticket_id}");
            return on_download_target(from_node_id, ticket_id).await;
        }

        // puller has download the ticket, we can safely remove it
        Some(CommAction::DownloadDone(from_node_id, ticket_id)) => {
            println!("action: DownloadDone: {from_node_id}, {ticket_id}");
            return on_download_done(from_node_id, ticket_id).await;
        }

        // puller requested the timestamp status of a target from a pusher
        Some(CommAction::RequestTargetTimestamp(from_node_id, target_name)) => {
            println!("action: RequestTargetTimestamp: {from_node_id}, {target_name}");
            return on_request_target_timestamp(from_node_id, target_name).await;
        }

        // pusher informs the timestamp status of a target to a puller
        Some(CommAction::TargetTimestamp(from_node_id, target_name, timestamp)) => {
            println!("action: TargetTimestamp: {from_node_id}, {target_name}, {timestamp}");
            return on_target_timestamp(from_node_id, target_name, timestamp).await;
        }

        // do nothing on extra not handled stuff
        _ => {}
    }

    Ok(())
}

async fn on_send_message(
    conn: &Arc<Mutex<Connection>>,
    to_node_id: String,
    msg: String,
) -> Result<()> {
    conn.lock().await.send_msg_to_node(to_node_id, msg).await
}

async fn on_target_has_changed(
    sync_process: &SyncProcess,
    actions_queue: &Arc<Mutex<queue::Queue<CommAction>>>,
    to_node_id: String,
    target_name: String,
) -> Result<()> {
    // get all the request target actions to request to the pusher
    let send_actions: Vec<CommAction> = sync_process
        .get_pull_targets_by_name(&target_name)
        .iter()
        .map(|target| {
            CommAction::RequestTarget(to_node_id.clone(), target.name.clone()).to_send_message()
        })
        .collect();

    if send_actions.is_empty() {
        return Ok(());
    }

    // cache the actions so that the event looper can send the requests
    actions_queue.lock().await.push_multiple(send_actions);

    Ok(())
}

async fn on_request_target(_from_node_id: String, _target_name: String) -> Result<()> {
    // let ticket_id = "".to_string();
    // let _actions = CommAction::to_download_targets(ticket_id, vec![node_id]);
    // TODO: do we have this target?!
    // TODO: check if msg is needed and if we want to download, if we want, request the ticket

    Ok(())
}

async fn on_download_target(_from_node_id: String, _ticket_id: String) -> Result<()> {
    // ...

    Ok(())
}

async fn on_download_done(_from_node_id: String, _ticket_id: String) -> Result<()> {
    // TODO: we need to think this through, it is possible that more nodes
    //       are still downloading. for now, leave it on the tmp storage

    Ok(())
}

async fn on_request_target_timestamp(
    _from_node_id: String,
    _target_name: String,
) -> Result<()> {
    // TODO: check the target current timestamp and see if we should sync
    Ok(())
}

async fn on_target_timestamp(
    _from_node_id: String,
    _target_name: String,
    _timestamp: DateTime<Utc>,
) -> Result<()> {
    // TODO: check the target current timestamp and see if we should sync
    Ok(())
}
