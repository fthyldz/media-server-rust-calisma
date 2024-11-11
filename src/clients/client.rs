use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite::Message;

use crate::webrtc::webrtc_manager::WebRTCClient;

pub struct Client {
    pub ws: UnboundedSender<Message>,
    pub pc: WebRTCClient,
}

impl Client {
    pub fn new(ws: UnboundedSender<Message>) -> Client {
        Client {
            ws,
            pc: WebRTCClient::new(),
        }
    }
}
