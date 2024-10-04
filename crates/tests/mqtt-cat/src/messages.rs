use crate::templates::matches;
use mqttrs::QoS;

#[derive(Clone)]
pub struct Message {
    pub topic: String,
    pub payload: String,
    pub qos: QoS,
    pub retain: bool,
}

pub struct MessageTemplate {
    pub topic: Option<String>,
    pub payload: Option<String>,
    pub qos: Option<QoS>,
    pub retain: Option<bool>,
}

impl MessageTemplate {
    pub fn matches(&self, message: &Message) -> bool {
        matches(&self.topic, &message.topic)
            && matches(&self.payload, &message.payload)
            && matches(&self.qos, &message.qos)
            && matches(&self.retain, &message.retain)
    }
}
