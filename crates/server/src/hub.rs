use common::WsServerEvent;
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct Hub {
    sender: broadcast::Sender<Arc<WsServerEvent>>,
}

impl Hub {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<WsServerEvent>> {
        self.sender.subscribe()
    }

    pub fn publish(&self, event: WsServerEvent) {
        let _ = self.sender.send(Arc::new(event));
    }
}
