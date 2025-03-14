use crate::cli::common::Cloud;
use crate::cli::disconnect::error::*;
use crate::cli::log::Fancy;
use crate::cli::log::Spinner;
use crate::command::*;
use crate::log::MaybeFancy;
use crate::system_services::*;
use anyhow::Context;
use std::sync::Arc;
use tedge_config::TEdgeConfigLocation;
use which::which;

const TEDGE_BRIDGE_CONF_DIR_PATH: &str = "mosquitto-conf";

#[derive(Debug)]
pub struct DisconnectBridgeCommand {
    pub config_location: TEdgeConfigLocation,
    pub cloud: Cloud,
    pub use_mapper: bool,
    pub service_manager: Arc<dyn SystemServiceManager>,
}

impl Command for DisconnectBridgeCommand {
    fn description(&self) -> String {
        format!("remove the bridge to disconnect {} cloud", self.cloud)
    }

    fn execute(&self) -> Result<(), MaybeFancy<anyhow::Error>> {
        match self.stop_bridge() {
            Ok(())
            | Err(Fancy {
                err: DisconnectBridgeError::BridgeFileDoesNotExist,
                ..
            }) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }
}

impl DisconnectBridgeCommand {
    fn service_manager(&self) -> &dyn SystemServiceManager {
        self.service_manager.as_ref()
    }

    fn stop_bridge(&self) -> Result<(), Fancy<DisconnectBridgeError>> {
        // If this fails, do not continue with applying changes and stopping/disabling tedge-mapper.
        let is_fatal_error = |err: &DisconnectBridgeError| {
            !matches!(err, DisconnectBridgeError::BridgeFileDoesNotExist)
        };
        let res = Spinner::start_filter_errors("Removing bridge config file", is_fatal_error)
            .finish(self.remove_bridge_config_file());
        if res
            .as_ref()
            .err()
            .filter(|e| !is_fatal_error(&e.err))
            .is_some()
        {
            println!(
                "Bridge doesn't exist. Device is already disconnected from {}.",
                self.cloud
            );
            return Ok(());
        } else {
            res?
        }

        if let Err(SystemServiceError::ServiceManagerUnavailable { cmd: _, name }) =
            self.service_manager.check_operational()
        {
            println!(
                "Service manager '{name}' is not available, skipping stopping/disabling of tedge components.",
            );
            return Ok(());
        }

        // Ignore failure
        let _ = self.apply_changes_to_mosquitto();

        // Only C8Y changes the status of tedge-mapper
        if self.use_mapper && which("tedge-mapper").is_ok() {
            let spinner = Spinner::start(format!("Disabling {}", self.cloud.mapper_service()));
            spinner.finish(
                self.service_manager()
                    .stop_and_disable_service(self.cloud.mapper_service()),
            )?;
        }

        Ok(())
    }

    fn remove_bridge_config_file(&self) -> Result<(), DisconnectBridgeError> {
        let config_file = self.cloud.bridge_config_filename();
        let bridge_conf_path = self
            .config_location
            .tedge_config_root_path
            .join(TEDGE_BRIDGE_CONF_DIR_PATH)
            .join(config_file.as_ref());

        match std::fs::remove_file(&bridge_conf_path) {
            // If we find the bridge config file we remove it
            // and carry on to see if we need to restart mosquitto.
            Ok(()) => Ok(()),

            // If bridge config file was not found we assume that the bridge doesn't exist,
            // We finish early returning exit code 0.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(DisconnectBridgeError::BridgeFileDoesNotExist)
            }

            Err(e) => Err(e).with_context(|| format!("Failed to delete {bridge_conf_path}"))?,
        }
    }

    // Deviation from specification:
    // Check if mosquitto is running, restart only if it was active before, if not don't do anything.
    fn apply_changes_to_mosquitto(&self) -> Result<bool, Fancy<DisconnectBridgeError>> {
        restart_service_if_running(&*self.service_manager, SystemService::Mosquitto)
            .map_err(<_>::into)
    }
}

fn restart_service_if_running(
    manager: &dyn SystemServiceManager,
    service: SystemService,
) -> Result<bool, Fancy<SystemServiceError>> {
    if manager.is_service_running(service)? {
        let spinner = Spinner::start("Restarting mosquitto to apply configuration");
        spinner.finish(manager.restart_service(service))?;
        Ok(true)
    } else {
        Ok(false)
    }
}
