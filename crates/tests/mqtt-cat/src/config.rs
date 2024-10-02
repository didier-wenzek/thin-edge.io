use mqttrs::QoS;
use mqttrs::SubscribeTopic;

#[derive(Clone)]
pub struct Config {
    pub host: String,
    pub keep_alive: u16,
    pub client_id: String,
    pub clean_session: bool,
    pub subscriptions: Vec<SubscribeTopic>,
    pub message_sample: Message,
}

#[derive(Clone)]
pub struct Message {
    pub topic: String,
    pub payload: String,
    pub qos: QoS,
    pub retain: bool,
}
