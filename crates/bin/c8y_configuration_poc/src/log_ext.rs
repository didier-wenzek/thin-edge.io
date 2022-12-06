use mqtt_channel::Message;
use tokio::sync::mpsc::{Receiver, Sender};

pub struct LogManagerActor {
    mqtt_sender_address: Sender<Message>,
    mqtt_receiver_address: Receiver<Message>,
}

impl LogManagerActor {
    pub fn new(
        mqtt_sender_address: Sender<Message>,
        mqtt_receiver_address: Receiver<Message>,
    ) -> Self {
        LogManagerActor {
            mqtt_sender_address,
            mqtt_receiver_address,
        }
    }
}
