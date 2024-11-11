use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use tokio::sync::RwLock;

use super::client::Client;

type Clients = Arc<RwLock<HashMap<SocketAddr, Arc<RwLock<Client>>>>>;

pub struct ClientManager {
    clients: Clients,
}

impl ClientManager {
    pub fn new() -> Arc<RwLock<ClientManager>> {
        Arc::new(RwLock::new(ClientManager {
            clients: Arc::new(RwLock::new(HashMap::new())),
        }))
    }

    pub async fn add_client(&self, addr: SocketAddr, client: Arc<RwLock<Client>>) {
        self.clients.write().await.insert(addr, client);
    }

    pub async fn remove_client(&self, addr: &SocketAddr) {
        self.clients.write().await.remove(addr);
    }

    pub async fn get_client(&self, addr: &SocketAddr) -> Option<Arc<RwLock<Client>>> {
        self.clients.read().await.get(addr).cloned()
    }

    pub async fn get_clients(&self) -> Clients {
        Arc::clone(&self.clients)
    }
}
