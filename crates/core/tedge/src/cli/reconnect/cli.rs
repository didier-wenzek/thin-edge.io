use super::command::ReconnectBridgeCommand;
use crate::cli::common::CloudArg;
use crate::command::*;
use crate::system_services::service_manager;
use tedge_config::TEdgeConfig;

#[derive(clap::Args, Debug)]
pub struct TEdgeReconnectCli {
    /// Ignore connection registration and connection check
    #[clap(long = "offline", global = true)]
    offline_mode: bool,

    #[clap(subcommand)]
    cloud: CloudArg,
}

impl BuildCommand for TEdgeReconnectCli {
    fn build_command(self, config: &TEdgeConfig) -> Result<Box<dyn Command>, crate::ConfigError> {
        Ok(ReconnectBridgeCommand {
            config_dir: config.root_dir().to_owned(),
            service_manager: service_manager(config.root_dir())?,
            cloud: self.cloud.try_into()?,
            offline_mode: self.offline_mode,
            use_mapper: true,
        }
        .into_boxed())
    }
}
