use crate::actions::Action;
use crate::actions::ActionTemplate;
use crate::actions::ActionTemplate::*;
use crate::config::Config;
use crate::events::Event;
use crate::events::EventPattern;
use crate::events::EventPattern::*;
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
                return templates
                    .iter()
                    .map(|template| template.build(session, config, event))
                    .collect();
            }
        }
        vec![]
    }

    pub fn sub_client() -> Self {
        let rules = vec![
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
                Received(Pubrec { pid: None }),
                vec![Send(Pubrel { pid: None })],
            ),
            (
                Received(Pubrel { pid: None }),
                vec![Send(Pubcomp { pid: None })],
            ),
        ];
        StateMachine { rules }
    }

    pub fn broker() -> Self {
        let rules = vec![
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
                Received(Pubrec { pid: None }),
                vec![Send(Pubrel { pid: None })],
            ),
            (
                Received(Pubrel { pid: None }),
                vec![Send(Pubcomp { pid: None })],
            ),
            (Received(Pingreq), vec![Send(Pingresp)]),
            (TcpDisconnected, vec![]),
        ];

        Self { rules }
    }
}
