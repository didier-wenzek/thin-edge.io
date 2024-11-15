use crate::messages::C8YRestError;
use crate::messages::CreateEvent;
use crate::messages::EventId;
use crate::messages::SoftwareListResponse;
use crate::C8YHttpConfig;
use crate::C8YHttpProxyActor;
use crate::C8YHttpProxyBuilder;
use c8y_api::http_proxy::InvalidUrl;
use c8y_api::json_c8y::C8yUpdateSoftwareListResponse;
use reqwest::Url;
use tedge_actors::Service;
use tedge_http_ext::HttpRequest;
use tedge_http_ext::HttpResult;

/// Facade over C8Y REST API
#[derive(Clone)]
pub struct C8YHttpProxy {
    c8y: C8YHttpProxyActor,
}

impl C8YHttpProxy {
    pub fn new(
        config: impl Into<C8YHttpConfig>,
        http: &mut impl Service<HttpRequest, HttpResult>,
    ) -> Self {
        let c8y = C8YHttpProxyBuilder::new(config.into(), http).build();
        C8YHttpProxy { c8y }
    }

    pub async fn connect(&mut self) -> Result<(), C8YRestError> {
        self.c8y.init().await?;
        Ok(())
    }

    // Return the local url going through the local auth proxy to reach the given remote url
    //
    // Return the remote url unchanged if not related to the current tenant.
    pub fn local_proxy_url(&self, remote_url: &str) -> Result<Url, InvalidUrl> {
        self.c8y.end_point.local_proxy_url(remote_url)
    }

    // Returns the c8y url to upload an attachment onto an event
    pub fn c8y_url_for_event_binary_upload(&self, event_id: &str) -> Url {
        self.c8y
            .end_point
            .get_url_for_event_binary_upload_unchecked(event_id)
    }

    // Returns the local url to upload an attachment onto an event
    pub fn proxy_url_for_event_binary_upload(&self, event_id: &str) -> Url {
        self.c8y
            .end_point
            .proxy_url_for_event_binary_upload(event_id)
    }

    pub async fn send_event(&mut self, c8y_event: CreateEvent) -> Result<EventId, C8YRestError> {
        self.c8y.init().await?;
        self.c8y.create_event(c8y_event).await
    }

    pub async fn send_software_list_http(
        &mut self,
        c8y_software_list: C8yUpdateSoftwareListResponse,
        device_id: String,
    ) -> Result<(), C8YRestError> {
        self.c8y.init().await?;
        let request = SoftwareListResponse {
            c8y_software_list,
            device_id,
        };

        self.c8y.send_software_list_http(request).await
    }
}
