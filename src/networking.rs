use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio;

#[derive(Clone)]
pub struct MockPeerManager {
    // Add a counter to track setup_inbound calls
    setup_count: Arc<std::sync::atomic::AtomicUsize>,
}

impl MockPeerManager {
    pub fn new() -> Self {
        MockPeerManager {
            setup_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    // Method to get the number of setup_inbound calls
    fn get_setup_count(&self) -> usize {
        self.setup_count.load(Ordering::SeqCst)
    }
}

// Mock setup_inbound function for testing purposes
async fn setup_inbound(peer_manager: Arc<MockPeerManager>, _stream: std::net::TcpStream) {
    // Increment the counter when called
    peer_manager.setup_count.fetch_add(1, Ordering::SeqCst);
    // Mock implementation (does nothing else)
}

// Define the networking function
pub async fn start_network_listener(
    peer_manager: Arc<MockPeerManager>,
    listening_port: u16,
) {
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(format!("[::]:{}", listening_port))
            .await
            .expect("Failed to bind to listen port - is something else already listening on it?");
        loop {
            let tcp_stream = listener.accept().await.unwrap().0;
            let peer_mgr = peer_manager.clone();
            tokio::spawn(async move {
                setup_inbound(
                    peer_mgr,
                    tcp_stream.into_std().unwrap(),
                )
                .await;
            });
        }
    });
}