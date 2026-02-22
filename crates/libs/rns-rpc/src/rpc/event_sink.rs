use super::{EventSinkBridge, RpcEventSinkEnvelope};
use serde::{Deserialize, Serialize};
use std::io;
use std::sync::Arc;

pub trait WebhookEventPublisher: Send + Sync {
    fn post_event(
        &self,
        config: &WebhookEventSinkConfig,
        envelope: &RpcEventSinkEnvelope,
    ) -> io::Result<()>;
}

pub trait MqttEventPublisher: Send + Sync {
    fn publish_event(
        &self,
        config: &MqttEventSinkConfig,
        envelope: &RpcEventSinkEnvelope,
    ) -> io::Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebhookEventSinkConfig {
    pub sink_id: String,
    pub endpoint: String,
    #[serde(default)]
    pub auth_header: Option<String>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MqttEventSinkConfig {
    pub sink_id: String,
    pub broker_uri: String,
    pub topic: String,
    #[serde(default)]
    pub qos: u8,
    #[serde(default)]
    pub retain: bool,
}

fn default_timeout_ms() -> u64 {
    5_000
}

pub struct WebhookEventSinkBridge {
    config: WebhookEventSinkConfig,
    publisher: Arc<dyn WebhookEventPublisher>,
}

impl WebhookEventSinkBridge {
    pub fn new(
        config: WebhookEventSinkConfig,
        publisher: Arc<dyn WebhookEventPublisher>,
    ) -> io::Result<Self> {
        let sink_id = config.sink_id.trim();
        if sink_id.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "webhook sink_id must not be empty",
            ));
        }
        if config.endpoint.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "webhook endpoint must not be empty",
            ));
        }
        if config.timeout_ms == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "webhook timeout_ms must be greater than zero",
            ));
        }
        Ok(Self { config, publisher })
    }

    pub fn config(&self) -> &WebhookEventSinkConfig {
        &self.config
    }
}

impl EventSinkBridge for WebhookEventSinkBridge {
    fn sink_id(&self) -> &str {
        self.config.sink_id.as_str()
    }

    fn sink_kind(&self) -> &'static str {
        "webhook"
    }

    fn publish(&self, envelope: &RpcEventSinkEnvelope) -> io::Result<()> {
        self.publisher.post_event(&self.config, envelope)
    }
}

pub struct MqttEventSinkBridge {
    config: MqttEventSinkConfig,
    publisher: Arc<dyn MqttEventPublisher>,
}

impl MqttEventSinkBridge {
    pub fn new(
        config: MqttEventSinkConfig,
        publisher: Arc<dyn MqttEventPublisher>,
    ) -> io::Result<Self> {
        let sink_id = config.sink_id.trim();
        if sink_id.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "mqtt sink_id must not be empty",
            ));
        }
        if config.broker_uri.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "mqtt broker_uri must not be empty",
            ));
        }
        if config.topic.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "mqtt topic must not be empty",
            ));
        }
        if config.qos > 2 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "mqtt qos must be 0, 1, or 2"));
        }
        Ok(Self { config, publisher })
    }

    pub fn config(&self) -> &MqttEventSinkConfig {
        &self.config
    }
}

impl EventSinkBridge for MqttEventSinkBridge {
    fn sink_id(&self) -> &str {
        self.config.sink_id.as_str()
    }

    fn sink_kind(&self) -> &'static str {
        "mqtt"
    }

    fn publish(&self, envelope: &RpcEventSinkEnvelope) -> io::Result<()> {
        self.publisher.publish_event(&self.config, envelope)
    }
}
