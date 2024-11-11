use std::{
    error::Error,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};

use futures::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc::UnboundedReceiver, Mutex, RwLock},
    task::JoinHandle,
};
use tokio_tungstenite::{accept_async, tungstenite::Message, WebSocketStream};

use crate::clients::{client::Client, client_manager::ClientManager};

pub async fn start_socket_server(
    client_manager: Arc<RwLock<ClientManager>>,
) -> Result<(), Box<dyn Error>> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8080);
    let listener = TcpListener::bind(&addr).await?;
    println!("WebSocket sunucusu şurada çalışıyor: {}", addr);

    while let Ok((stream, addr)) = listener.accept().await {
        println!("Yeni bağlantı: {}", addr);

        let client_manager = Arc::clone(&client_manager);
        tokio::spawn(handle_connection(client_manager, stream, addr));
    }

    Ok(())
}

async fn handle_connection(
    client_manager: Arc<RwLock<ClientManager>>,
    stream: TcpStream,
    addr: SocketAddr,
) {
    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            println!("WebSocket hatası: {}", e);
            return;
        }
    };

    let (ws_sender, ws_receiver) = ws_stream.split();

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Message>();

    client_manager
        .write()
        .await
        .add_client(addr, Arc::new(RwLock::new(Client::new(tx))))
        .await;

    create_broadcast(rx, ws_sender).await;

    handle_messages(Arc::clone(&client_manager), ws_receiver, addr).await;
}

async fn create_broadcast(
    mut rx: UnboundedReceiver<Message>,
    mut ws_sender: SplitSink<WebSocketStream<TcpStream>, Message>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let _ = ws_sender.send(msg).await;
        }
    })
}

#[derive(Serialize, Deserialize, Debug)]
enum MessageType {
    Chat,
    Offer,
    Answer,
    Candidate,
}

async fn handle_messages(
    client_manager: Arc<RwLock<ClientManager>>,
    mut ws_receiver: SplitStream<WebSocketStream<TcpStream>>,
    addr: SocketAddr,
) {
    while let Some(result) = ws_receiver.next().await {
        match result {
            Ok(msg) => match msg {
                Message::Text(text) => match serde_json::from_str::<Value>(&text) {
                    Ok(msg) => {
                        if let Some(msg_type) = msg
                            .get("msg_type")
                            .map(|v| serde_json::from_value::<MessageType>(v.clone()))
                        {
                            match msg_type {
                                Ok(msg_type) => match msg_type {
                                    MessageType::Chat => {
                                        chat_message(Arc::clone(&client_manager), msg, addr).await;
                                    }
                                    MessageType::Offer => {
                                        offer_message(Arc::clone(&client_manager), msg, addr).await;
                                    }
                                    MessageType::Answer => {
                                        chat_message(Arc::clone(&client_manager), msg, addr).await;
                                    }
                                    MessageType::Candidate => {
                                        candidate_message(Arc::clone(&client_manager), msg, addr)
                                            .await;
                                    }
                                },
                                _ => {
                                    println!("Bilinmeyen msg_type: {:?}", msg_type);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        println!("JSON ayrıştırma hatası: {}", e);
                    }
                },
                Message::Close(_) => {
                    println!("{} bağlantıyı kapatıyor", addr);
                    break;
                }
                _ => {}
            },
            Err(e) => {
                println!("Mesaj hatası: {}", e);
                break;
            }
        }
    }

    println!("{} bağlantısı koptu", addr);
    client_manager.write().await.remove_client(&addr).await;
}

#[derive(Serialize, Deserialize, Debug)]
struct ChatMessage {
    msg_type: String,
    content: String,
}

async fn chat_message(client_manager: Arc<RwLock<ClientManager>>, msg: Value, addr: SocketAddr) {
    if let Ok(chat_msg) = serde_json::from_value::<ChatMessage>(msg.clone()) {
        for (client_addr, client_tx) in client_manager
            .read()
            .await
            .get_clients()
            .await
            .write()
            .await
            .iter()
        {
            if *client_addr != addr {
                let json_msg = serde_json::to_string(&chat_msg).unwrap();
                if let Err(e) = client_tx.write().await.ws.send(Message::Text(json_msg)) {
                    println!("Broadcast hatası {}: {}", client_addr, e);
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct OfferSignalMessage {
    msg_type: String,
    content: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct AnswerSignalMessage {
    msg_type: String,
    content: String,
}

async fn offer_message(client_manager: Arc<RwLock<ClientManager>>, msg: Value, addr: SocketAddr) {
    if let Ok(offer_msg) = serde_json::from_value::<OfferSignalMessage>(msg.clone()) {
        let remote_sdp_string = offer_msg.content;
        if let Some(client) = client_manager.read().await.get_client(&addr).await {
            let client_clone = Arc::clone(&client);
            let mut client = client_clone.write().await;
            let ws = Arc::new(Mutex::new(client.ws.clone()));
            if client.pc.get_peer_connection_is_none() {
                client.pc.create_peer_connection().await;
                client.pc.gather_and_send_candidates(ws).await;
                client.pc.on_track().await;
            }
            client
                .pc
                .set_remote_description(remote_sdp_string, true)
                .await;
            let answer_sdp = client.pc.create_answer().await.unwrap();
            let answer_msg = OfferSignalMessage {
                msg_type: "Answer".to_string(),
                content: answer_sdp,
            };

            let json_msg = serde_json::to_string(&answer_msg).unwrap();
            if let Err(e) = client.ws.send(Message::Text(json_msg)) {
                println!("Cevap gönderme hatası: {}", e);
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct IceCandidateSignalMessage {
    msg_type: String,
    content: String,
    sdp_mid: Option<String>,
    sdp_mline_index: Option<u16>,
    username_fragment: Option<String>,
}

async fn candidate_message(
    client_manager: Arc<RwLock<ClientManager>>,
    msg: Value,
    addr: SocketAddr,
) {
    if let Ok(candidate_msg) = serde_json::from_value::<IceCandidateSignalMessage>(msg.clone()) {
        if let Some(client) = client_manager.read().await.get_client(&addr).await {
            let mut client = client.write().await;
            client
                .pc
                .add_ice_candidate(
                    candidate_msg.content,
                    candidate_msg.sdp_mid,
                    candidate_msg.sdp_mline_index,
                    candidate_msg.username_fragment,
                )
                .await;
        }
    }
}
