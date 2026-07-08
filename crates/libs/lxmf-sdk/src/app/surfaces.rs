use super::delivery::{DeliveryOptions, DeliveryPlan, SendReport};
use super::discovery::{
    BootstrapRequest, Contact, ContactPage, ContactUpdate, Identity, PeerDirectoryEntry,
    PresencePage,
};
use super::errors::Error;
#[cfg(feature = "sdk-async")]
use super::events::SubscriptionStart;
use super::node::Client;
use super::runtime::{Config, DeliveryStatus, Handle, RuntimeStatus, SendReceipt, SendRequest};
#[cfg(feature = "sdk-async")]
use super::session::EventStream;
use crate::domain::{
    AttachmentId, AttachmentListRequest, AttachmentListResult, AttachmentMeta,
    AttachmentStoreRequest,
};
#[cfg(feature = "std")]
use crate::error::{code, ErrorCategory as SdkErrorCategory, SdkError};
#[cfg(feature = "std")]
use crate::messaging::{
    ConversationListPage, ConversationListRequest, MessageHistoryListRequest, MessageHistoryPage,
};
use crate::CancelResult;
use crate::{SdkBackend, ShutdownMode};
#[cfg(feature = "sdk-async")]
use crate::{SdkBackendAsyncEvents, SdkBackendAsyncOps};

pub struct Messages<'a, B: SdkBackend> {
    client: &'a Client<B>,
}

impl<'a, B: SdkBackend> Messages<'a, B> {
    pub(crate) fn new(client: &'a Client<B>) -> Self {
        Self { client }
    }

    pub fn send(&self, request: SendRequest) -> Result<SendReceipt, Error> {
        self.client.send(request)
    }

    #[cfg(feature = "sdk-async")]
    pub async fn send_async(&self, request: SendRequest) -> Result<SendReceipt, Error>
    where
        B: SdkBackendAsyncOps,
    {
        self.client.send_async(request).await
    }

    pub fn send_with_profile_defaults(&self, request: SendRequest) -> Result<SendReport, Error> {
        self.client.send_with_profile_defaults(request)
    }

    #[cfg(feature = "sdk-async")]
    pub async fn send_with_profile_defaults_async(
        &self,
        request: SendRequest,
    ) -> Result<SendReport, Error>
    where
        B: SdkBackendAsyncOps,
    {
        self.client.send_with_profile_defaults_async(request).await
    }

    pub fn send_with_options(
        &self,
        request: SendRequest,
        options: DeliveryOptions,
    ) -> Result<SendReport, Error> {
        self.client.send_with_options(request, options)
    }

    #[cfg(feature = "sdk-async")]
    pub async fn send_with_options_async(
        &self,
        request: SendRequest,
        options: DeliveryOptions,
    ) -> Result<SendReport, Error>
    where
        B: SdkBackendAsyncOps,
    {
        self.client.send_with_options_async(request, options).await
    }

    pub fn status(
        &self,
        message_id: impl Into<crate::MessageId>,
    ) -> Result<Option<DeliveryStatus>, Error> {
        self.client.delivery_status(message_id)
    }

    pub fn cancel(&self, message_id: impl Into<crate::MessageId>) -> Result<CancelResult, Error> {
        self.client.backend.cancel(message_id.into()).map_err(Error::from)
    }

    #[cfg(feature = "sdk-async")]
    pub async fn status_async(
        &self,
        message_id: impl Into<crate::MessageId>,
    ) -> Result<Option<DeliveryStatus>, Error>
    where
        B: SdkBackendAsyncOps,
    {
        self.client.delivery_status_async(message_id).await
    }

    pub fn delivery_plan(&self) -> Result<DeliveryPlan, Error> {
        self.client.delivery_plan()
    }

    #[cfg(feature = "std")]
    pub fn history(&self, request: MessageHistoryListRequest) -> Result<MessageHistoryPage, Error> {
        let payload = serde_json::to_value(request).map_err(app_internal_error)?;
        let response = self.client.query("app.message.history.list", payload)?;
        serde_json::from_value(response.payload).map_err(app_internal_error)
    }

    #[cfg(feature = "std")]
    pub fn conversations(
        &self,
        request: ConversationListRequest,
    ) -> Result<ConversationListPage, Error> {
        let payload = serde_json::to_value(request).map_err(app_internal_error)?;
        let response = self.client.query("app.message.conversation.list", payload)?;
        serde_json::from_value(response.payload).map_err(app_internal_error)
    }
}

#[cfg(feature = "std")]
fn app_internal_error(err: serde_json::Error) -> Error {
    Error::from(SdkError::new(code::INTERNAL, SdkErrorCategory::Internal, err.to_string()))
}

#[cfg(feature = "sdk-async")]
pub struct Events<'a, B: SdkBackend> {
    client: &'a Client<B>,
}

#[cfg(feature = "sdk-async")]
impl<'a, B: SdkBackendAsyncEvents> Events<'a, B> {
    pub(crate) fn new(client: &'a Client<B>) -> Self {
        Self { client }
    }

    pub fn subscribe(&self, start: SubscriptionStart) -> Result<EventStream<B>, Error> {
        self.client.subscribe_events(start)
    }
}

pub struct IdentityDirectory<'a, B: SdkBackend> {
    client: &'a Client<B>,
}

impl<'a, B: SdkBackend> IdentityDirectory<'a, B> {
    pub(crate) fn new(client: &'a Client<B>) -> Self {
        Self { client }
    }

    pub fn list(&self) -> Result<Vec<Identity>, Error> {
        self.client.identities()
    }

    pub fn announce_now(&self) -> Result<(), Error> {
        self.client.announce_now()
    }

    pub fn contacts(
        &self,
        cursor: Option<String>,
        limit: Option<usize>,
    ) -> Result<ContactPage, Error> {
        self.client.contacts(cursor, limit)
    }

    pub fn presence(
        &self,
        cursor: Option<String>,
        limit: Option<usize>,
    ) -> Result<PresencePage, Error> {
        self.client.presence(cursor, limit)
    }

    pub fn presence_since(
        &self,
        cursor: Option<String>,
        limit: Option<usize>,
        min_last_seen_ts_ms: Option<i64>,
    ) -> Result<PresencePage, Error> {
        self.client.presence_since(cursor, limit, min_last_seen_ts_ms)
    }

    pub fn update_contact(&self, update: ContactUpdate) -> Result<Contact, Error> {
        self.client.update_contact(update)
    }

    pub fn bootstrap(&self, request: BootstrapRequest) -> Result<Contact, Error> {
        self.client.bootstrap_identity(request)
    }

    pub fn peer_directory(&self, limit: Option<usize>) -> Result<Vec<PeerDirectoryEntry>, Error> {
        self.client.peer_directory(limit)
    }

    pub fn peer_directory_since(
        &self,
        limit: Option<usize>,
        min_last_seen_ts_ms: Option<i64>,
    ) -> Result<Vec<PeerDirectoryEntry>, Error> {
        self.client.peer_directory_since(limit, min_last_seen_ts_ms)
    }
}

pub struct Attachments<'a, B: SdkBackend> {
    client: &'a Client<B>,
}

impl<'a, B: SdkBackend> Attachments<'a, B> {
    pub(crate) fn new(client: &'a Client<B>) -> Self {
        Self { client }
    }

    pub fn store(&self, request: AttachmentStoreRequest) -> Result<AttachmentMeta, Error> {
        self.client.backend.attachment_store(request).map_err(Error::from)
    }

    pub fn get(&self, attachment_id: AttachmentId) -> Result<Option<AttachmentMeta>, Error> {
        self.client.backend.attachment_get(attachment_id).map_err(Error::from)
    }

    pub fn list(&self, request: AttachmentListRequest) -> Result<AttachmentListResult, Error> {
        self.client.backend.attachment_list(request).map_err(Error::from)
    }

    pub fn delete(&self, attachment_id: AttachmentId) -> Result<(), Error> {
        self.client.backend.attachment_delete(attachment_id).map_err(Error::from)?;
        Ok(())
    }
}

pub struct Runtime<'a, B: SdkBackend> {
    client: &'a Client<B>,
}

impl<'a, B: SdkBackend> Runtime<'a, B> {
    pub(crate) fn new(client: &'a Client<B>) -> Self {
        Self { client }
    }

    pub fn start(&self, config: Config) -> Result<Handle, Error> {
        self.client.start(config)
    }

    #[cfg(feature = "sdk-async")]
    pub async fn start_async(&self, config: Config) -> Result<Handle, Error>
    where
        B: SdkBackendAsyncOps,
    {
        self.client.start_async(config).await
    }

    pub fn restart(&self, config: Config) -> Result<Handle, Error> {
        self.client.restart(config)
    }

    #[cfg(feature = "sdk-async")]
    pub async fn restart_async(&self, config: Config) -> Result<Handle, Error>
    where
        B: SdkBackendAsyncOps,
    {
        self.client.stop_async(ShutdownMode::Immediate).await?;
        self.client.start_async(config).await
    }

    pub fn status(&self) -> Result<RuntimeStatus, Error> {
        self.client.status()
    }

    #[cfg(feature = "sdk-async")]
    pub async fn status_async(&self) -> Result<RuntimeStatus, Error>
    where
        B: SdkBackendAsyncOps,
    {
        self.client.status_async().await
    }

    pub fn stop(&self, mode: ShutdownMode) -> Result<(), Error> {
        self.client.stop(mode)
    }

    #[cfg(feature = "sdk-async")]
    pub async fn stop_async(&self, mode: ShutdownMode) -> Result<(), Error>
    where
        B: SdkBackendAsyncOps,
    {
        self.client.stop_async(mode).await
    }
}
