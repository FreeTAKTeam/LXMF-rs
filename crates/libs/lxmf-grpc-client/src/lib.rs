#![allow(clippy::result_large_err)]

use std::time::Duration;

use thiserror::Error;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity as TlsIdentity};
use tonic::{Request, Status};

pub mod lxmf {
    pub mod common {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.common.v1.rs"));
        }
    }

    pub mod runtime {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.runtime.v1.rs"));
        }
    }

    pub mod delivery {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.delivery.v1.rs"));
        }
    }

    pub mod command {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.command.v1.rs"));
        }
    }

    pub mod admin {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.admin.v1.rs"));
        }
    }

    pub mod topics {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.topics.v1.rs"));
        }
    }

    pub mod attachments {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.attachments.v1.rs"));
        }
    }

    pub mod events {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.events.v1.rs"));
        }
    }

    pub mod identity {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.identity.v1.rs"));
        }
    }

    pub mod markers {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.markers.v1.rs"));
        }
    }

    pub mod peers {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/lxmf.peers.v1.rs"));
        }
    }
}

pub use lxmf::admin::v1::interface_admin_service_client::InterfaceAdminServiceClient;
pub use lxmf::attachments::v1::attachment_service_client::AttachmentServiceClient;
pub use lxmf::command::v1::command_service_client::CommandServiceClient;
pub use lxmf::delivery::v1::delivery_service_client::DeliveryServiceClient;
pub use lxmf::events::v1::event_service_client::EventServiceClient;
pub use lxmf::identity::v1::identity_service_client::IdentityServiceClient;
pub use lxmf::markers::v1::marker_service_client::MarkerServiceClient;
pub use lxmf::peers::v1::peer_service_client::PeerServiceClient;
pub use lxmf::runtime::v1::runtime_service_client::RuntimeServiceClient;
pub use lxmf::topics::v1::topic_service_client::TopicServiceClient;

pub type ClientTransport = InterceptedService<Channel, AuthInterceptor>;

#[derive(Clone, Default)]
pub struct AuthInterceptor {
    bearer_token: Option<MetadataValue<Ascii>>,
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        if let Some(token) = self.bearer_token.clone() {
            request.metadata_mut().insert("authorization", token);
        }
        Ok(request)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ClientTlsSettings {
    pub domain_name: Option<String>,
    pub ca_certificate_pem: Option<Vec<u8>>,
    pub client_cert_pem: Option<Vec<u8>>,
    pub client_key_pem: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct ClientBuilder {
    endpoint: String,
    bearer_token: Option<String>,
    timeout: Option<Duration>,
    tls: Option<ClientTlsSettings>,
}

impl ClientBuilder {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            bearer_token: None,
            timeout: Some(Duration::from_secs(10)),
            tls: None,
        }
    }

    pub fn bearer_token(mut self, bearer_token: impl Into<String>) -> Self {
        self.bearer_token = Some(bearer_token.into());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn tls(mut self, tls: ClientTlsSettings) -> Self {
        self.tls = Some(tls);
        self
    }

    pub async fn connect(self) -> Result<LxmfGrpcClient, ClientBuildError> {
        let mut endpoint = Endpoint::from_shared(self.endpoint)?;
        if let Some(timeout) = self.timeout {
            endpoint = endpoint.timeout(timeout);
        }
        if let Some(tls) = self.tls {
            let mut config = ClientTlsConfig::new();
            if let Some(domain_name) = tls.domain_name {
                config = config.domain_name(domain_name);
            }
            if let Some(ca_certificate_pem) = tls.ca_certificate_pem {
                config = config.ca_certificate(Certificate::from_pem(ca_certificate_pem));
            }
            if let (Some(client_cert_pem), Some(client_key_pem)) =
                (tls.client_cert_pem, tls.client_key_pem)
            {
                config = config.identity(TlsIdentity::from_pem(client_cert_pem, client_key_pem));
            }
            endpoint = endpoint.tls_config(config)?;
        }

        let channel = endpoint.connect().await?;
        let bearer_token = self.bearer_token.as_deref().map(normalize_bearer_token).transpose()?;

        Ok(LxmfGrpcClient { channel, interceptor: AuthInterceptor { bearer_token } })
    }
}

#[derive(Clone)]
pub struct LxmfGrpcClient {
    channel: Channel,
    interceptor: AuthInterceptor,
}

impl LxmfGrpcClient {
    pub fn builder(endpoint: impl Into<String>) -> ClientBuilder {
        ClientBuilder::new(endpoint)
    }

    pub async fn connect(endpoint: impl Into<String>) -> Result<Self, ClientBuildError> {
        Self::builder(endpoint).connect().await
    }

    pub fn runtime(&self) -> RuntimeServiceClient<ClientTransport> {
        RuntimeServiceClient::with_interceptor(self.channel.clone(), self.interceptor.clone())
    }

    pub fn admin(&self) -> InterfaceAdminServiceClient<ClientTransport> {
        InterfaceAdminServiceClient::with_interceptor(
            self.channel.clone(),
            self.interceptor.clone(),
        )
    }

    pub fn delivery(&self) -> DeliveryServiceClient<ClientTransport> {
        DeliveryServiceClient::with_interceptor(self.channel.clone(), self.interceptor.clone())
    }

    pub fn commands(&self) -> CommandServiceClient<ClientTransport> {
        CommandServiceClient::with_interceptor(self.channel.clone(), self.interceptor.clone())
    }

    pub fn topics(&self) -> TopicServiceClient<ClientTransport> {
        TopicServiceClient::with_interceptor(self.channel.clone(), self.interceptor.clone())
    }

    pub fn attachments(&self) -> AttachmentServiceClient<ClientTransport> {
        AttachmentServiceClient::with_interceptor(self.channel.clone(), self.interceptor.clone())
    }

    pub fn events(&self) -> EventServiceClient<ClientTransport> {
        EventServiceClient::with_interceptor(self.channel.clone(), self.interceptor.clone())
    }

    pub fn identity(&self) -> IdentityServiceClient<ClientTransport> {
        IdentityServiceClient::with_interceptor(self.channel.clone(), self.interceptor.clone())
    }

    pub fn markers(&self) -> MarkerServiceClient<ClientTransport> {
        MarkerServiceClient::with_interceptor(self.channel.clone(), self.interceptor.clone())
    }

    pub fn peers(&self) -> PeerServiceClient<ClientTransport> {
        PeerServiceClient::with_interceptor(self.channel.clone(), self.interceptor.clone())
    }
}

fn normalize_bearer_token(raw: &str) -> Result<MetadataValue<Ascii>, ClientBuildError> {
    let value = if raw.trim_start().starts_with("Bearer ") {
        raw.trim().to_string()
    } else {
        format!("Bearer {}", raw.trim())
    };
    MetadataValue::try_from(value).map_err(ClientBuildError::InvalidAuthMetadata)
}

#[derive(Debug, Error)]
pub enum ClientBuildError {
    #[error("invalid endpoint: {0}")]
    InvalidEndpoint(#[from] tonic::transport::Error),
    #[error("invalid authorization metadata")]
    InvalidAuthMetadata(#[from] tonic::metadata::errors::InvalidMetadataValue),
}
