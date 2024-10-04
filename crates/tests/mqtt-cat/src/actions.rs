use crate::config::Config;
use crate::events::Event;
use crate::events::ExpectedEvent;
use crate::events::ExpectedEventTemplate;
use crate::session::Session;
use crate::templates::PacketTemplate;
use mqttrs::Packet;
use mqttrs::Pid;

pub enum Action<'a> {
    Nop,
    Send(Packet<'a>),
    TriggerTimer { pid: Pid, expected: ExpectedEvent },
    ClearTimer { pid: Pid },
}

pub enum ActionTemplate {
    Send(PacketTemplate),
    TriggerTimer { expected: ExpectedEventTemplate },
    ClearTimer { expected: ExpectedEventTemplate },
}

impl ActionTemplate {
    pub fn build<'a>(
        &'a self,
        session: &mut Session,
        config: &'a Config,
        event: &'a Event,
    ) -> Action<'a> {
        match self {
            ActionTemplate::Send(packet) => Action::Send(packet.build(session, config, event)),
            ActionTemplate::TriggerTimer { expected } => match expected {
                ExpectedEventTemplate::MessageAck => match event {
                    Event::MessageQueued(message) => {
                        let pid = session.next_pid();
                        let expected = ExpectedEvent::MessageAck(message.clone());
                        Action::TriggerTimer { pid, expected }
                    }
                    _ => Action::Nop,
                },
                ExpectedEventTemplate::MessageRec => match event {
                    Event::MessageQueued(message) => {
                        let pid = session.next_pid();
                        let expected = ExpectedEvent::MessageRec(message.clone());
                        Action::TriggerTimer { pid, expected }
                    }
                    _ => Action::Nop,
                },
                ExpectedEventTemplate::MessageComp => match event.pid() {
                    None => Action::Nop,
                    Some(pid) => {
                        let pid = *pid;
                        let expected = ExpectedEvent::MessageComp;
                        Action::TriggerTimer { pid, expected }
                    }
                },
            },
            ActionTemplate::ClearTimer { .. } => match event.pid() {
                None => Action::Nop,
                Some(pid) => {
                    let pid = *pid;
                    Action::ClearTimer { pid }
                }
            },
        }
    }
}
