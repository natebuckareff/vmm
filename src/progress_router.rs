use anyhow::Result;
use tokio::sync::{broadcast, mpsc};

use crate::task_group::TaskGroup;

pub fn create_progress_router(task_group: &mut TaskGroup<Result<()>>) -> ProgressRouterClient {
    let (mpsc_sender, mpsc_receiver) = mpsc::channel(100);
    let (broadcast_sender, _) = broadcast::channel(100);

    let progress_tracker = ProgressRouter::new(broadcast_sender.clone(), mpsc_receiver);

    task_group.spawn(async move {
        progress_tracker.run().await.unwrap();
        Ok(())
    });

    ProgressRouterClient::new(mpsc_sender, broadcast_sender)
}

#[derive(Debug, Clone)]
pub enum ProgressMessage {
    Start(String, Option<u64>),
    Update(String, u64),
    Finish(String),
}

#[derive(Clone)]
pub struct ProgressRouterClient {
    sender: mpsc::Sender<ProgressMessage>,
    sender_broadcast: broadcast::Sender<ProgressMessage>,
}

impl ProgressRouterClient {
    pub fn new(
        sender: mpsc::Sender<ProgressMessage>,
        sender_broadcast: broadcast::Sender<ProgressMessage>,
    ) -> Self {
        Self {
            sender,
            sender_broadcast,
        }
    }

    pub async fn send(&self, message: ProgressMessage) {
        let _ = self.sender.send(message).await;
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ProgressMessage> {
        self.sender_broadcast.subscribe()
    }
}

pub struct ProgressRouter {
    sender: broadcast::Sender<ProgressMessage>,
    receiver: mpsc::Receiver<ProgressMessage>,
}

impl ProgressRouter {
    pub fn new(
        sender: broadcast::Sender<ProgressMessage>,
        receiver: mpsc::Receiver<ProgressMessage>,
    ) -> Self {
        Self { sender, receiver }
    }

    pub async fn run(mut self) -> Result<()> {
        while let Some(message) = self.receiver.recv().await {
            let _ = self.sender.send(message);
        }
        Ok(())
    }
}
