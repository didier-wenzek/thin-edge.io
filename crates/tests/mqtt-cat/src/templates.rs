use crate::config::Config;
use crate::events::Event;
use crate::session::Session;
use mqttrs::ConnectReturnCode;
use mqttrs::Packet;
use mqttrs::Pid;
use mqttrs::QoS;
use mqttrs::QosPid;
use mqttrs::SubscribeReturnCodes;
use mqttrs::SubscribeTopic;
use crate::Message;

pub enum PacketTemplate {
    Connect {
        keep_alive: Option<u16>,
        client_id: Option<String>,
        clean_session: Option<bool>,
    },
    Connack {
        session_present: Option<bool>,
        code: Option<ConnectReturnCode>,
    },
    Publish {
        dup: Option<bool>,
        qos: Option<QoS>,
        pid: Option<Pid>,
        retain: Option<bool>,
        topic: Option<String>,
        payload: Option<String>,
    },
    Puback {
        pid: Option<Pid>,
    },
    Pubrec {
        pid: Option<Pid>,
    },
    Pubrel {
        pid: Option<Pid>,
    },
    Pubcomp {
        pid: Option<Pid>,
    },
    Subscribe {
        pid: Option<Pid>,
        subscriptions: Vec<SubscribeTopic>,
    },
    Suback {
        pid: Option<Pid>,
        codes: Vec<SubscribeReturnCodes>,
    },
    Unsubscribe {
        pid: Option<Pid>,
        subscriptions: Vec<String>,
    },
    Unsuback {
        pid: Option<Pid>,
    },
    Pingreq,
    Pingresp,
    Disconnect,
}

impl PacketTemplate {
    pub fn matches(&self, packet: &Packet) -> bool {
        match (self, packet) {
            (
                PacketTemplate::Connect {
                    keep_alive,
                    client_id,
                    clean_session,
                },
                Packet::Connect(connect),
            ) => {
                matches(keep_alive, &connect.keep_alive)
                    && matches(client_id, &connect.client_id.to_string())
                    && matches(clean_session, &connect.clean_session)
            }

            (
                PacketTemplate::Connack {
                    session_present,
                    code,
                },
                Packet::Connack(connack),
            ) => matches(session_present, &connack.session_present) && matches(code, &connack.code),

            (
                PacketTemplate::Publish {
                    dup,
                    qos,
                    pid,
                    retain,
                    topic,
                    ..
                },
                Packet::Publish(publish),
            ) => {
                matches(dup, &publish.dup)
                    && matches(qos, &publish.qospid.qos())
                    && (if pid.is_some() {
                        pid == &publish.qospid.pid()
                    } else {
                        true
                    })
                    && matches(retain, &publish.retain)
                    && matches(topic, &publish.topic_name)
            }

            (PacketTemplate::Puback { pid }, Packet::Puback(received_pid)) => {
                matches(pid, received_pid)
            }

            (PacketTemplate::Pubrec { pid }, Packet::Pubrec(received_pid)) => {
                matches(pid, received_pid)
            }

            (PacketTemplate::Pubrel { pid }, Packet::Pubrel(received_pid)) => {
                matches(pid, received_pid)
            }

            (PacketTemplate::Pubcomp { pid }, Packet::Pubcomp(received_pid)) => {
                matches(pid, received_pid)
            }

            (PacketTemplate::Subscribe { pid, .. }, Packet::Subscribe(subscribe)) => {
                matches(pid, &subscribe.pid)
                // TODO check the subscriptions
            }

            (PacketTemplate::Suback { pid, .. }, Packet::Suback(suback)) => {
                matches(pid, &suback.pid)
                // TODO check the return codes
            }

            (PacketTemplate::Unsubscribe { pid, .. }, Packet::Unsubscribe(unsubscribe)) => {
                matches(pid, &unsubscribe.pid)
                // TODO check the subscriptions
            }

            (PacketTemplate::Unsuback { pid }, Packet::Unsuback(received_pid)) => {
                matches(pid, received_pid)
            }

            (PacketTemplate::Pingreq, Packet::Pingreq)
            | (PacketTemplate::Pingresp, Packet::Pingresp)
            | (PacketTemplate::Disconnect, Packet::Disconnect) => true,

            (_, _) => false,
        }
    }

    pub fn build<'a>(&'a self, given_pid: Pid, config: &'a Config, event: &'a Event) -> Packet<'a> {
        match self {
            PacketTemplate::Connect {
                keep_alive,
                client_id,
                clean_session,
            } => Packet::Connect(mqttrs::Connect {
                protocol: mqttrs::Protocol::MQTT311,
                keep_alive: keep_alive.unwrap_or(config.keep_alive),
                client_id: client_id.as_ref().unwrap_or(&config.client_id),
                clean_session: clean_session.unwrap_or(config.clean_session),
                last_will: None,
                username: None,
                password: None,
            }),
            PacketTemplate::Connack {
                session_present,
                code,
            } => Packet::Connack(mqttrs::Connack {
                session_present: session_present.unwrap_or(false),
                code: code.unwrap_or(ConnectReturnCode::Accepted),
            }),
            PacketTemplate::Publish {
                dup,
                qos,
                pid,
                retain,
                topic,
                payload,
            } => {
                let message = match event {
                    Event::MessageQueued(message) => message,
                    _ => &config.message_sample,
                };
                let qos = qos.unwrap_or(message.qos);
                let pid = pid.unwrap_or(given_pid);
                Packet::Publish(mqttrs::Publish {
                    dup: dup.unwrap_or(false),
                    qospid: qospid(qos, pid),
                    retain: retain.unwrap_or(message.retain),
                    topic_name: topic.as_ref().unwrap_or(&message.topic),
                    payload: payload
                        .as_ref()
                        .unwrap_or(&message.payload)
                        .as_bytes(),
                })
            }
            PacketTemplate::Puback { pid } => Packet::Puback(pid.unwrap_or(given_pid)),
            PacketTemplate::Pubrec { pid } => Packet::Pubrec(pid.unwrap_or(given_pid)),
            PacketTemplate::Pubrel { pid } => Packet::Pubrel(pid.unwrap_or(given_pid)),
            PacketTemplate::Pubcomp { pid } => Packet::Pubcomp(pid.unwrap_or(given_pid)),
            PacketTemplate::Subscribe { pid, subscriptions } => {
                let topics = if subscriptions.is_empty() {
                    &config.subscriptions
                } else {
                    subscriptions
                };
                Packet::Subscribe(mqttrs::Subscribe {
                    pid: pid.unwrap_or(given_pid),
                    topics: topics.clone(),
                })
            }
            PacketTemplate::Suback { pid, codes } => Packet::Suback(mqttrs::Suback {
                pid: pid.unwrap_or(given_pid),
                return_codes: codes.clone(),
            }),
            PacketTemplate::Unsubscribe { pid, subscriptions } => {
                let topics = if subscriptions.is_empty() {
                    config
                        .subscriptions
                        .iter()
                        .map(|sub| sub.topic_path.clone())
                        .collect()
                } else {
                    subscriptions.clone()
                };
                Packet::Unsubscribe(mqttrs::Unsubscribe {
                    pid: pid.unwrap_or(given_pid),
                    topics,
                })
            }
            PacketTemplate::Unsuback { pid } => Packet::Unsuback(pid.unwrap_or(given_pid)),
            PacketTemplate::Pingreq => Packet::Pingreq,
            PacketTemplate::Pingresp => Packet::Pingresp,
            PacketTemplate::Disconnect => Packet::Disconnect,
        }
    }
}

pub fn matches<T, U>(template: &Option<T>, u: &U) -> bool
where
    T: PartialEq<U>,
{
    template.as_ref().map(|t| t == u).unwrap_or(true)
}

fn qospid(qos: QoS, pid: Pid) -> QosPid {
    match qos {
        QoS::AtMostOnce => QosPid::AtMostOnce,
        QoS::AtLeastOnce => QosPid::AtLeastOnce(pid),
        QoS::ExactlyOnce => QosPid::ExactlyOnce(pid),
    }
}
