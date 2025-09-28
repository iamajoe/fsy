#[derive(Debug, Clone)]
pub enum CommAction {
    // SendMessage used to send messages through the connection to a node
    SendMessage(String, String),

    // ReceiveMessage used to receive messages through the connection from a node
    ReceiveMessage(String, String),
}
