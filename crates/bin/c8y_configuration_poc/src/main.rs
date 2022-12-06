mod c8y_http_proxy;
mod config_manager;
mod file_system_ext;
mod log_ext;
mod mqtt_ext;

use crate::config_manager::{ConfigConfigManager, ConfigManager};
use mqtt_channel::Config;
use mqtt_ext::MqttActorBuilder;
use std::path::PathBuf;
use tedge_actors::Runtime;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let runtime_events_logger = None;
    let mut runtime = Runtime::try_new(runtime_events_logger).await?;

    // Create actor instances
    let mut mqtt_actor_builder = MqttActorBuilder::new(Config::default());
    let mut http_actor =
        tedge_http_ext::HttpActorInstance::new(tedge_http_ext::HttpConfig::default())?;
    let mut config_actor =
        ConfigManager::new(ConfigConfigManager::from_tedge_config("/etc/tedge")?);

    // Connect actor instances
    config_actor.with_http_connection(&mut http_actor)?;
    // Fetch Receiver<MqttMessage> from the mqtt actor
    let mqtt_handles = mqtt_actor_builder.register_peer(
        vec![
            "c8y/s/us",
            "tedge/+/commands/res/config_snapshot",
            "tedge/+/commands/res/config_update",
        ]
        .try_into()
        .unwrap(),
    );

    // Run the actors
    runtime.spawn(mqtt_actor_builder).await?;
    runtime.spawn(http_actor).await?;
    runtime.spawn(config_actor).await?;

    runtime.run_to_completion().await?;
    Ok(())
}
