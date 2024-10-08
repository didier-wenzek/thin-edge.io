use crate::actions::Action;
use crate::actions::ActionTemplate;
use crate::actions::ActionTemplate::*;
use crate::config::Config;
use crate::events::Event;
use crate::events::EventPattern;
use crate::events::EventPattern::*;
use crate::events::ExpectedEventTemplate;
use crate::messages::MessageTemplate;
use crate::session::Session;
use crate::templates::PacketTemplate::*;
use mqttrs::ConnectReturnCode::Accepted;
use mqttrs::QoS;

pub struct StateMachine {
    /// on event matching a given pattern => list of actions
    rules: Vec<(EventPattern, Vec<ActionTemplate>)>,
}

impl StateMachine {
    pub fn derive_actions<'a>(
        &'a self,
        session: &mut Session,
        config: &'a Config,
        event: &'a Event,
    ) -> Vec<Action<'a>> {
        for (pattern, templates) in self.rules.iter() {
            if pattern.matches(event) {
                let mut pid = event.pid();
                return templates
                    .iter()
                    .map(|template| template.build(&mut pid, session, config, event))
                    .collect();
            }
        }
        vec![]
    }

    pub fn sub_client() -> Self {
        let mut rules = Self::mqtt_rules();
        rules.append(&mut vec![
            (
                TcpConnected,
                vec![Send(Connect {
                    keep_alive: None,
                    client_id: None,
                    clean_session: None,
                })],
            ),
            (
                Received(Connack {
                    session_present: None,
                    code: None,
                }),
                vec![Send(Subscribe {
                    pid: None,
                    subscriptions: vec![],
                })],
            ),
        ]);
        StateMachine { rules }
    }

    pub fn broker() -> Self {
        let mut rules = Self::mqtt_rules();
        rules.append(&mut vec![
            (TcpConnected, vec![]),
            (
                Received(Connect {
                    keep_alive: None,
                    client_id: None,
                    clean_session: None,
                }),
                vec![Send(Connack {
                    session_present: Some(false),
                    code: Some(Accepted),
                })],
            ),
            (
                Received(Subscribe {
                    pid: None,
                    subscriptions: vec![],
                }),
                vec![Send(Suback {
                    pid: None,
                    codes: vec![],
                })],
            ),
            (Received(Pingreq), vec![Send(Pingresp)]),
            (TcpDisconnected, vec![]),
        ]);

        Self { rules }
    }

    fn mqtt_rules() -> Vec<(EventPattern, Vec<ActionTemplate>)> {
        vec![
            (
                Received(Publish {
                    dup: None,
                    qos: Some(QoS::AtMostOnce),
                    pid: None,
                    retain: None,
                    topic: None,
                    payload: None,
                }),
                vec![],
            ),
            (
                Received(Publish {
                    dup: None,
                    qos: Some(QoS::AtLeastOnce),
                    pid: None,
                    retain: None,
                    topic: None,
                    payload: None,
                }),
                vec![Send(Puback { pid: None })],
            ),
            (
                Received(Publish {
                    dup: None,
                    qos: Some(QoS::ExactlyOnce),
                    pid: None,
                    retain: None,
                    topic: None,
                    payload: None,
                }),
                vec![Send(Pubrec { pid: None })],
            ),
            (
                MessageQueued(MessageTemplate {
                    topic: None,
                    payload: None,
                    qos: Some(QoS::AtMostOnce),
                    retain: None,
                }),
                vec![Send(Publish {
                    dup: Some(false),
                    qos: Some(QoS::AtMostOnce),
                    pid: None,
                    retain: None,
                    topic: None,
                    payload: None,
                })],
            ),
            (
                MessageQueued(MessageTemplate {
                    topic: None,
                    payload: None,
                    qos: Some(QoS::AtLeastOnce),
                    retain: None,
                }),
                vec![
                    GeneratePid,
                    TriggerTimer {
                        expected: ExpectedEventTemplate::MessageAck,
                    },
                    Send(Publish {
                        dup: Some(false),
                        qos: None,
                        pid: None,
                        retain: None,
                        topic: None,
                        payload: None,
                    }),
                ],
            ),
            (
                Timeout(ExpectedEventTemplate::MessageAck),
                vec![
                    TriggerTimer {
                        expected: ExpectedEventTemplate::MessageAck,
                    },
                    Send(Publish {
                        dup: Some(true),
                        qos: None,
                        pid: None,
                        retain: None,
                        topic: None,
                        payload: None,
                    }),
                ],
            ),
            (
                Received(Puback { pid: None }),
                vec![ClearTimer {
                    expected: ExpectedEventTemplate::MessageAck,
                }],
            ),
            (
                MessageQueued(MessageTemplate {
                    topic: None,
                    payload: None,
                    qos: Some(QoS::ExactlyOnce),
                    retain: None,
                }),
                vec![
                    GeneratePid,
                    TriggerTimer {
                        expected: ExpectedEventTemplate::MessageRec,
                    },
                    Send(Publish {
                        dup: Some(false),
                        qos: None,
                        pid: None,
                        retain: None,
                        topic: None,
                        payload: None,
                    }),
                ],
            ),
            (
                Timeout(ExpectedEventTemplate::MessageRec),
                vec![
                    TriggerTimer {
                        expected: ExpectedEventTemplate::MessageRec,
                    },
                    Send(Publish {
                        dup: Some(true),
                        qos: None,
                        pid: None,
                        retain: None,
                        topic: None,
                        payload: None,
                    }),
                ],
            ),
            (
                Received(Pubrec { pid: None }),
                vec![
                    ClearTimer {
                        expected: ExpectedEventTemplate::MessageRec,
                    },
                    TriggerTimer {
                        expected: ExpectedEventTemplate::MessageComp,
                    },
                    Send(Pubrel { pid: None }),
                ],
            ),
            (
                Timeout(ExpectedEventTemplate::MessageComp),
                vec![
                    TriggerTimer {
                        expected: ExpectedEventTemplate::MessageComp,
                    },
                    Send(Pubrel { pid: None }),
                ],
            ),
            (
                Received(Pubcomp { pid: None }),
                vec![ClearTimer {
                    expected: ExpectedEventTemplate::MessageComp,
                }],
            ),
            (
                Received(Pubrel { pid: None }),
                vec![Send(Pubcomp { pid: None })],
            ),
        ]
    }
}
