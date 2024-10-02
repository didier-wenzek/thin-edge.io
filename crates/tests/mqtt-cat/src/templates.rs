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
    Subscribe {
        pid: Option<Pid>,
        subscriptions: Vec<SubscribeTopic>,
    },
    Suback {
        pid: Option<Pid>,
        codes: Vec<SubscribeReturnCodes>,
    },
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
            (_, _) => false,
        }
    }

    pub fn build<'a>(
        &'a self,
        session: &mut Session,
        config: &'a Config,
        event: &'a Event,
    ) -> Packet<'a> {
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
                let qos = qos.unwrap_or(config.message_sample.qos);
                let pid = most_specific_pid(pid, event, session);
                Packet::Publish(mqttrs::Publish {
                    dup: dup.unwrap_or(false),
                    qospid: qospid(qos, pid),
                    retain: retain.unwrap_or(config.message_sample.retain),
                    topic_name: topic.as_ref().unwrap_or(&config.message_sample.topic),
                    payload: payload
                        .as_ref()
                        .unwrap_or(&config.message_sample.payload)
                        .as_bytes(),
                })
            }
            PacketTemplate::Puback { pid } => {
                Packet::Puback(most_specific_pid(pid, event, session))
            }
            PacketTemplate::Subscribe { pid, subscriptions } => {
                let topics = if subscriptions.is_empty() {
                    &config.subscriptions
                } else {
                    subscriptions
                };
                Packet::Subscribe(mqttrs::Subscribe {
                    pid: most_specific_pid(pid, event, session),
                    topics: topics.clone(),
                })
            }
            PacketTemplate::Suback { pid, codes } => Packet::Suback(mqttrs::Suback {
                pid: most_specific_pid(pid, event, session),
                return_codes: codes.clone(),
            }),
        }
    }
}

fn matches<T, U>(template: &Option<T>, u: &U) -> bool
where
    T: PartialEq<U>,
{
    template.as_ref().map(|t| t == u).unwrap_or(true)
}

fn most_specific_pid(pid: &Option<Pid>, event: &Event, session: &mut Session) -> mqttrs::Pid {
    if let Some(pid) = pid {
        return *pid;
    }
    if let Some(pid) = event.pid() {
        return *pid;
    }
    session.next_pid()
}

fn qospid(qos: QoS, pid: Pid) -> QosPid {
    match qos {
        QoS::AtMostOnce => QosPid::AtMostOnce,
        QoS::AtLeastOnce => QosPid::AtLeastOnce(pid),
        QoS::ExactlyOnce => QosPid::ExactlyOnce(pid),
    }
}
