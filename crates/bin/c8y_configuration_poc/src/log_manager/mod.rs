mod actor;
mod config;
mod error;

use crate::c8y_http_proxy::handle::C8YHttpProxy;
use crate::c8y_http_proxy::C8YConnectionBuilder;
use crate::c8y_http_proxy::C8YHttpProxyBuilder;
use crate::file_system_ext::FsWatchActorBuilder;
use actor::*;
use async_trait::async_trait;
pub use config::*;
use tedge_actors::mpsc;
use tedge_actors::ActorBuilder;
use tedge_actors::DynSender;
use tedge_actors::LinkError;
use tedge_actors::MessageBoxConnector;
use tedge_actors::MessageBoxPort;
use tedge_actors::RuntimeError;
use tedge_actors::RuntimeHandle;
use tedge_actors::Sender;
use tedge_mqtt_ext::*;

/// This is an actor builder.
pub struct LogManagerBuilder {
    config: LogManagerConfig,
    events_receiver: mpsc::Receiver<LogInput>,
    events_sender: mpsc::Sender<LogInput>,
    mqtt_publisher: Option<DynSender<MqttMessage>>,
    http_proxy: Option<C8YHttpProxy>,
}

impl LogManagerBuilder {
    pub fn new(config: LogManagerConfig) -> Self {
        let (events_sender, events_receiver) = mpsc::channel(10);

        Self {
            config,
            events_receiver,
            events_sender,
            mqtt_publisher: None,
            http_proxy: None,
        }
    }

    /// Connect this config manager instance to some http connection provider
    pub fn with_c8y_http_proxy(&mut self, http: &mut C8YHttpProxyBuilder) -> Result<(), LinkError> {
        self.http_proxy = Some(http.new_c8y_handle("LogManager => C8Y"));
        Ok(())
    }

    /// Connect this config manager instance to some mqtt connection provider
    pub fn with_mqtt_connection(&mut self, mqtt: &mut MqttActorBuilder) -> Result<(), LinkError> {
        let subscriptions = vec!["c8y/s/ds"].try_into().unwrap();
        mqtt.connect_with(self, subscriptions);
        Ok(())
    }

    pub fn with_fs_connection(
        &mut self,
        fs_builder: &mut FsWatchActorBuilder,
    ) -> Result<(), LinkError> {
        let config_dir = self.config.config_dir.clone();
        fs_builder.new_watcher(config_dir, self.events_sender.clone().into());

        Ok(())
    }
}

impl MessageBoxPort<MqttMessage, MqttMessage> for LogManagerBuilder {
    fn set_request_sender(&mut self, mqtt_publisher: DynSender<MqttMessage>) {
        self.mqtt_publisher = Some(mqtt_publisher);
    }

    fn get_response_sender(&self) -> DynSender<MqttMessage> {
        self.events_sender.sender_clone()
    }
}

#[async_trait]
impl ActorBuilder for LogManagerBuilder {
    async fn spawn(self, runtime: &mut RuntimeHandle) -> Result<(), RuntimeError> {
        let mqtt_publisher = self.mqtt_publisher.ok_or_else(|| LinkError::MissingPeer {
            role: "mqtt".to_string(),
        })?;

        let http_proxy = self.http_proxy.ok_or_else(|| LinkError::MissingPeer {
            role: "http".to_string(),
        })?;

        let message_box = LogManagerMessageBox::new(self.events_receiver, mqtt_publisher.clone());

        let actor = LogManagerActor::new(self.config, mqtt_publisher, http_proxy);

        runtime.run(actor, message_box).await?;
        Ok(())
    }
}
