use crate::config::Config;
use crate::events::Event;
use crate::session::Session;
use crate::templates::PacketTemplate;
use mqttrs::Packet;

pub enum Action<'a> {
    Send(Packet<'a>),
}

pub enum ActionTemplate {
    Send(PacketTemplate),
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
        }
    }
}
