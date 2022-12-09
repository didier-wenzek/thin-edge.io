use crate::{Address, ChannelError, DynSender, Message, Sender};
use async_trait::async_trait;

/// A sender that adds a key to messages on the fly
pub struct KeyedSender<K: Message + Clone, M: Message> {
    key: K,
    address: Address<(K, M)>,
}

impl<K: Message + Clone, M: Message> KeyedSender<K, M> {
    pub fn new_sender(key: K, address: Address<(K, M)>) -> DynSender<M> {
        Box::new(KeyedSender { key, address })
    }
}

#[async_trait]
impl<K: Message + Clone, M: Message> Sender<M> for KeyedSender<K, M> {
    async fn send(&mut self, message: M) -> Result<(), ChannelError> {
        self.address.send((self.key.clone(), message)).await
    }

    fn sender_clone(&self) -> DynSender<M> {
        Box::new(KeyedSender {
            key: self.key.clone(),
            address: self.address.clone(),
        })
    }
}

/// A vector of senders addressed using a sender id attached to each message
pub struct SenderVec<M: Message> {
    senders: Vec<DynSender<M>>,
}

impl<M: Message> SenderVec<M> {
    pub fn new_sender(senders: Vec<DynSender<M>>) -> DynSender<(usize, M)> {
        Box::new(SenderVec { senders })
    }
}

#[async_trait]
impl<M: Message> Sender<(usize, M)> for SenderVec<M> {
    async fn send(&mut self, idx_message: (usize, M)) -> Result<(), ChannelError> {
        let (idx, message) = idx_message;
        if let Some(sender) = self.senders.get_mut(idx) {
            sender.send(message).await?;
        }
        Ok(())
    }

    fn sender_clone(&self) -> DynSender<(usize, M)> {
        let senders = self.senders.iter().map(|r| r.sender_clone()).collect();
        Box::new(SenderVec { senders })
    }
}
