use crate::c8y_http_proxy::messages::C8YRestError;
use crate::c8y_http_proxy::messages::C8YRestRequest;
use crate::c8y_http_proxy::messages::C8YRestResponse;
use crate::c8y_http_proxy::messages::C8YRestResult;
use crate::c8y_http_proxy::messages::UploadConfigFile;
use crate::c8y_http_proxy::messages::UploadLogBinary;
use crate::c8y_http_proxy::C8YConnectionBuilder;
use c8y_api::json_c8y::C8yCreateEvent;
use c8y_api::json_c8y::C8yUpdateSoftwareListResponse;
use mqtt_channel::StreamExt;
use std::path::Path;
use std::path::PathBuf;
use tedge_actors::mpsc;
use tedge_actors::DynSender;
use tedge_utils::file::PermissionEntry;

use super::messages::DownloadFile;

/// Handle to the C8YHttpProxy
pub struct C8YHttpProxy {
    request_sender: DynSender<C8YRestRequest>,
    response_receiver: mpsc::Receiver<C8YRestResult>,
}

impl C8YHttpProxy {
    /// Create a new handle to the C8YHttpProxy actor
    pub fn new(proxy: &mut (impl C8YConnectionBuilder + ?Sized)) -> C8YHttpProxy {
        // At most one response is expected
        let (response_sender, response_receiver) = mpsc::channel(1);

        let request_sender = proxy.connect(response_sender.into());
        C8YHttpProxy {
            request_sender,
            response_receiver,
        }
    }

    pub async fn send_event(&mut self, c8y_event: C8yCreateEvent) -> Result<String, C8YRestError> {
        self.request_sender.send(c8y_event.into()).await?;
        match self.response_receiver.next().await {
            Some(Ok(C8YRestResponse::EventId(id))) => Ok(id),
            unexpected => Err(unexpected.into()),
        }
    }

    pub async fn send_software_list_http(
        &mut self,
        c8y_software_list: C8yUpdateSoftwareListResponse,
    ) -> Result<(), C8YRestError> {
        self.request_sender.send(c8y_software_list.into()).await?;
        match self.response_receiver.next().await {
            Some(Ok(C8YRestResponse::Unit(_))) => Ok(()),
            unexpected => Err(unexpected.into()),
        }
    }

    pub async fn upload_log_binary(
        &mut self,
        log_type: &str,
        log_content: &str,
        child_device_id: Option<String>,
    ) -> Result<String, C8YRestError> {
        let request = UploadLogBinary {
            log_type: log_type.to_string(),
            log_content: log_content.to_string(),
            child_device_id,
        };
        self.request_sender.send(request.into()).await?;
        match self.response_receiver.next().await {
            Some(Ok(C8YRestResponse::EventId(id))) => Ok(id),
            unexpected => Err(unexpected.into()),
        }
    }

    pub async fn upload_config_file(
        &mut self,
        config_path: &Path,
        config_type: &str,
        child_device_id: Option<String>,
    ) -> Result<String, C8YRestError> {
        let request = UploadConfigFile {
            config_path: config_path.to_owned(),
            config_type: config_type.to_string(),
            child_device_id,
        };
        self.request_sender.send(request.into()).await?;
        match self.response_receiver.next().await {
            Some(Ok(C8YRestResponse::EventId(id))) => Ok(id),
            unexpected => Err(unexpected.into()),
        }
    }

    pub async fn download_file(
        &mut self,
        download_url: &str,
        file_path: PathBuf,
        file_permissions: PermissionEntry,
    ) -> Result<(), C8YRestError> {
        let request = DownloadFile {
            download_url: download_url.into(),
            file_path,
            file_permissions,
        };
        self.request_sender.send(request.into()).await?;
        match self.response_receiver.next().await {
            Some(Ok(C8YRestResponse::Unit(()))) => Ok(()),
            unexpected => Err(unexpected.into()),
        }
    }
}
