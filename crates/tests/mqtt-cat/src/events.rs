use crate::messages::*;
use crate::templates::PacketTemplate;
use mqttrs::Packet;
use mqttrs::Pid;
use mqttrs::QosPid;

pub enum Event<'a> {
    TcpConnected,
    TcpDisconnected,
    Received(Packet<'a>),
    MessageQueued(Message),
    Timeout { pid: Pid, expected: ExpectedEvent },
}

#[derive(Clone)]
pub enum ExpectedEvent {
    MessageAck(Message),
    MessageRec(Message),
    MessageComp,
}

pub enum EventPattern {
    TcpConnected,
    TcpDisconnected,
    Received(PacketTemplate),
    MessageQueued(MessageTemplate),
    Timeout(ExpectedEventTemplate),
}

pub enum ExpectedEventTemplate {
    MessageAck,
    MessageRec,
    MessageComp,
}

impl EventPattern {
    pub fn matches(&self, event: &Event) -> bool {
        match (self, event) {
            (EventPattern::TcpConnected, Event::TcpConnected) => true,
            (EventPattern::TcpDisconnected, Event::TcpDisconnected) => true,
            (EventPattern::Received(template), Event::Received(packet)) => template.matches(packet),
            (EventPattern::MessageQueued(template), Event::MessageQueued(message)) => {
                template.matches(message)
            }
            (EventPattern::Timeout(template), Event::Timeout { expected, .. }) => {
                template.matches(expected)
            }
            (_, _) => false,
        }
    }
}

impl<'a> Event<'a> {
    pub fn pid(&self) -> Option<&mqttrs::Pid> {
        match self {
            Event::TcpConnected => None,
            Event::TcpDisconnected => None,

            Event::Received(Packet::Connect(_)) => None,
            Event::Received(Packet::Connack(_)) => None,
            Event::Received(Packet::Publish(mqttrs::Publish {
                qospid: QosPid::AtMostOnce,
                ..
            })) => None,
            Event::Received(Packet::Publish(mqttrs::Publish {
                qospid: QosPid::AtLeastOnce(pid),
                ..
            })) => Some(pid),
            Event::Received(Packet::Publish(mqttrs::Publish {
                qospid: QosPid::ExactlyOnce(pid),
                ..
            })) => Some(pid),
            Event::Received(Packet::Puback(pid)) => Some(pid),
            Event::Received(Packet::Pubrec(pid)) => Some(pid),
            Event::Received(Packet::Pubrel(pid)) => Some(pid),
            Event::Received(Packet::Pubcomp(pid)) => Some(pid),
            Event::Received(Packet::Subscribe(mqttrs::Subscribe { pid, .. })) => Some(pid),
            Event::Received(Packet::Suback(mqttrs::Suback { pid, .. })) => Some(pid),
            Event::Received(Packet::Unsubscribe(mqttrs::Unsubscribe { pid, .. })) => Some(pid),
            Event::Received(Packet::Unsuback(pid)) => Some(pid),
            Event::Received(Packet::Pingreq) => None,
            Event::Received(Packet::Pingresp) => None,
            Event::Received(Packet::Disconnect) => None,

            Event::MessageQueued(_) => None,

            Event::Timeout { pid, .. } => Some(pid),
        }
    }
}

impl ExpectedEventTemplate {
    pub fn matches(&self, expected: &ExpectedEvent) -> bool {
        match (self, expected) {
            (ExpectedEventTemplate::MessageAck, ExpectedEvent::MessageAck(_))
            | (ExpectedEventTemplate::MessageRec, ExpectedEvent::MessageRec(_))
            | (ExpectedEventTemplate::MessageComp, ExpectedEvent::MessageComp) => true,

            (_, _) => false,
        }
    }
}
