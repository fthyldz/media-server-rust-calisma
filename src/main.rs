mod clients;
mod web_socket;
mod webrtc;

use clients::client_manager::ClientManager;
use web_socket::web_socket_manager::start_socket_server;

#[tokio::main]
async fn main() {
    let client_manager = ClientManager::new();

    let _ = start_socket_server(client_manager).await;
}
