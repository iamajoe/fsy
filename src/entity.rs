#[derive(Debug, Clone)]
pub enum CommAction {
    None,
    SendMessage(String, String),
    ReceiveMessage(String, String),
}
