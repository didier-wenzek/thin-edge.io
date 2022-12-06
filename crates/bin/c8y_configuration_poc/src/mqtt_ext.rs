use async_trait::async_trait;
use mqtt_channel::{Message, MqttError, SinkExt, StreamExt, TopicFilter};
use tedge_actors::{
    Actor, ActorBuilder, ChannelError, DynSender, Mailbox, RuntimeError, RuntimeHandle,
};
use tokio::sync::mpsc::{channel, Receiver, Sender};

pub struct MqttActorBuilder {
    pub mqtt_config: mqtt_channel::Config,
    pub publish_channel: (Sender<Message>, Receiver<Message>),
    pub subscriber_addresses: Vec<(TopicFilter, Sender<Message>)>,
}

impl MqttActorBuilder {
    pub fn new(config: mqtt_channel::Config) -> Self {
        MqttActorBuilder {
            mqtt_config: config,
            publish_channel: channel(10),
            subscriber_addresses: Vec::new(),
        }
    }

    pub fn register_peer(&mut self, topics: TopicFilter) -> (Sender<Message>, Receiver<Message>) {
        let (sender, receiver) = channel(10);
        self.subscriber_addresses.push((topics, sender));
        (self.publish_channel.0.clone(), receiver)
    }
}

#[async_trait]
impl ActorBuilder for MqttActorBuilder {
    async fn spawn(self, runtime: &mut RuntimeHandle) -> Result<(), RuntimeError> {
        let mut combined_topic_filter = TopicFilter::empty();
        for (topic_filter, _) in self.subscriber_addresses.iter() {
            combined_topic_filter.add_all(topic_filter.to_owned());
        }
        let mqtt_config = self.mqtt_config.with_subscriptions(combined_topic_filter);
        let mqtt_actor = MqttActor::new(
            mqtt_config,
            self.publish_channel.1,
            self.subscriber_addresses,
        )
        .await
        .unwrap(); // Convert MqttError to RuntimeError

        // mqtt_actor.run();
        Ok(())
    }
}

struct MqttActor {
    mqtt_client: mqtt_channel::Connection,
    mailbox: Receiver<Message>,
    peer_senders: Vec<(TopicFilter, Sender<Message>)>,
}

impl MqttActor {
    async fn new(
        mqtt_config: mqtt_channel::Config,
        mailbox: Receiver<Message>,
        peer_senders: Vec<(TopicFilter, Sender<Message>)>,
    ) -> Result<Self, MqttError> {
        let mqtt_client = mqtt_channel::Connection::new(&mqtt_config).await?;
        Ok(MqttActor {
            mqtt_client,
            mailbox,
            peer_senders,
        })
    }
}

#[async_trait]
impl Actor for MqttActor {
    type Input = Message;
    type Output = Message;
    type Mailbox = Mailbox<Message>;
    type Peers = DynSender<Self::Output>;

    async fn run(
        self,
        mut pub_messages: Self::Mailbox,
        mut sub_messages: Self::Peers,
    ) -> Result<(), ChannelError> {
        loop {
            tokio::select! {
                Some(message) = self.mailbox.recv() => {
                    self.mqtt_client.published.send(message);
                },
                Some(message) = self.mqtt_client.received.next() => {
                    for (topic_filter, peer_sender) in self.peer_senders.iter() {
                        if topic_filter.accept(&message) {
                            peer_sender.send(message);
                        }
                    }
                },
                else => return Ok(()),
            }
        }
    }
}
