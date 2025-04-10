use crate::command::Command;
use crate::log::MaybeFancy;
use tedge_config::tedge_toml::WritableKey;
use tedge_config::TEdgeConfigLocation;

pub struct UnsetConfigCommand {
    pub key: WritableKey,
    pub config_location: TEdgeConfigLocation,
}

#[async_trait::async_trait]
impl Command for UnsetConfigCommand {
    fn description(&self) -> String {
        format!("unset the configuration value for '{}'", self.key)
    }

    async fn execute(&self) -> Result<(), MaybeFancy<anyhow::Error>> {
        self.config_location
            .update_toml(&|dto, _reader| Ok(dto.try_unset_key(&self.key)?))
            .await
            .map_err(anyhow::Error::new)?;
        tracing::info!(target: "Audit", "tedge config unset {}", &self.key);
        Ok(())
    }
}
