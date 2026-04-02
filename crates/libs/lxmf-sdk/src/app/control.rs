use super::capabilities::CapabilitySummary;
use super::delivery::{
    AttemptDecision, AttemptDisposition, DeliveryAttempt, DeliveryOptions, DeliveryPlan, SendReport,
};
use super::errors::Error;
#[cfg(feature = "sdk-async")]
use super::events::{subscription_cursor, SubscriptionStart};
use super::node::Client;
use super::operations::OperationRegistry;
use super::runtime::{
    map_delivery_snapshot, map_runtime_state, Config, DeliveryStatus, Handle, RunState,
    RuntimeStatus, SendReceipt, SendRequest,
};
#[cfg(feature = "sdk-async")]
use super::session::EventStream;
use super::session::{SessionState, SharedBackend};
use crate::{Client as CoreClient, LxmfSdk, SdkBackend, ShutdownMode};
#[cfg(feature = "sdk-async")]
use crate::{LxmfSdkAsync, SdkBackendAsyncEvents};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

impl<B: SdkBackend> Client<B> {
    fn active_config(&self) -> Result<Config, Error> {
        let state = self.state.lock().expect("app client mutex poisoned");
        let Some(session) = state.session.as_ref() else {
            return Err(Error::not_started());
        };
        let session = session.lock().expect("app session mutex poisoned");
        Ok(session.config.clone())
    }

    pub fn delivery_plan(&self) -> Result<DeliveryPlan, Error> {
        Ok(self.active_config()?.delivery_plan())
    }

    pub fn operation_registry(&self) -> Result<OperationRegistry, Error> {
        match self.active_config() {
            Ok(config) => config.operation_registry().map_err(Error::from),
            Err(err) if matches!(err.code, super::errors::ErrorCode::RuntimeNotStarted) => {
                Ok(OperationRegistry::built_in().clone())
            }
            Err(err) => Err(err),
        }
    }

    pub fn start(&self, config: Config) -> Result<Handle, Error> {
        let mut state = self.state.lock().expect("app client mutex poisoned");
        if state.client.is_none() && state.session.is_some() {
            state.session = None;
            state.lifecycle = RunState::Stopped;
        }
        if let Some(session) = state.session.as_ref() {
            let session = session.lock().expect("app session mutex poisoned");
            return if session.config == config {
                Ok(Handle {
                    runtime_id: session.handle.runtime_id.clone(),
                    profile: session.config.profile.clone(),
                    capabilities: CapabilitySummary::from(&session.handle),
                })
            } else {
                Err(Error::already_running_different_config())
            };
        }

        state.lifecycle = RunState::Starting;
        let client = Arc::new(CoreClient::new(SharedBackend(Arc::clone(&self.backend))));
        let handle = match client.start(config.start_request()) {
            Ok(handle) => handle,
            Err(err) => {
                state.lifecycle = RunState::New;
                return Err(Error::from(err));
            }
        };
        let session = Arc::new(Mutex::new(SessionState {
            handle: handle.clone(),
            config: config.clone(),
            degraded: false,
        }));
        state.client = Some(Arc::clone(&client));
        state.session = Some(session);
        state.lifecycle = RunState::Running;

        Ok(Handle {
            runtime_id: handle.runtime_id.clone(),
            profile: config.profile,
            capabilities: CapabilitySummary::from(&handle),
        })
    }

    pub fn restart(&self, config: Config) -> Result<Handle, Error> {
        self.stop(ShutdownMode::Immediate)?;
        self.start(config)
    }

    pub fn send(&self, request: SendRequest) -> Result<SendReceipt, Error> {
        let state = self.state.lock().expect("app client mutex poisoned");
        let Some(client) = state.client.as_ref() else {
            return Err(Error::not_started());
        };
        let Some(session) = state.session.as_ref() else {
            return Err(Error::not_started());
        };
        let session = session.lock().expect("app session mutex poisoned");
        let correlation_id = request.correlation_id.clone();
        let message_id = client.send(request.into_raw()).map_err(Error::from)?;
        Ok(SendReceipt {
            runtime_id: session.handle.runtime_id.clone(),
            message_id: message_id.0,
            profile: session.config.profile.clone(),
            correlation_id,
        })
    }

    pub fn send_with_profile_defaults(&self, request: SendRequest) -> Result<SendReport, Error> {
        self.send_with_options(request, DeliveryOptions::default())
    }

    pub fn send_with_options(
        &self,
        request: SendRequest,
        options: DeliveryOptions,
    ) -> Result<SendReport, Error> {
        let plan = self.delivery_plan()?;
        let resolved = plan.resolve(&options);
        let started = Instant::now();
        let mut attempts: Vec<DeliveryAttempt> = Vec::new();
        let mut request = request;

        loop {
            let attempt_no = attempts.len() as u32 + 1;
            match self.send(request.clone()) {
                Ok(receipt) => {
                    let total_delay_ms = attempts
                        .iter()
                        .filter_map(|attempt: &DeliveryAttempt| attempt.scheduled_delay_ms)
                        .sum::<u64>();
                    return Ok(SendReport { receipt, attempts, total_delay_ms, plan });
                }
                Err(err) => match resolved.classify_failure(
                    attempt_no,
                    &err,
                    started.elapsed().as_millis() as u64,
                ) {
                    AttemptDecision::Retry(delay_ms) => {
                        attempts.push(DeliveryAttempt {
                            attempt: attempt_no,
                            disposition: AttemptDisposition::Retried,
                            error_code: err.code.as_str().to_owned(),
                            retryable: err.retryable,
                            queue_pressure: matches!(
                                err.code,
                                super::errors::ErrorCode::DeliveryQueuePressure
                            ),
                            scheduled_delay_ms: Some(delay_ms),
                        });
                        if delay_ms > 0 {
                            std::thread::sleep(Duration::from_millis(delay_ms));
                        }
                        request = request.clone();
                    }
                    AttemptDecision::Stop(stop_err) | AttemptDecision::Timeout(stop_err) => {
                        attempts.push(DeliveryAttempt {
                            attempt: attempt_no,
                            disposition: AttemptDisposition::Failed,
                            error_code: err.code.as_str().to_owned(),
                            retryable: err.retryable,
                            queue_pressure: matches!(
                                err.code,
                                super::errors::ErrorCode::DeliveryQueuePressure
                            ),
                            scheduled_delay_ms: None,
                        });
                        return Err(stop_err);
                    }
                },
            }
        }
    }

    pub fn delivery_status(
        &self,
        message_id: impl Into<crate::MessageId>,
    ) -> Result<Option<DeliveryStatus>, Error> {
        let state = self.state.lock().expect("app client mutex poisoned");
        let Some(client) = state.client.as_ref() else {
            return Err(Error::not_started());
        };
        let snapshot = client.status(message_id.into()).map_err(Error::from)?;
        Ok(snapshot.map(map_delivery_snapshot))
    }

    pub fn status(&self) -> Result<RuntimeStatus, Error> {
        let state = self.state.lock().expect("app client mutex poisoned");
        let Some(client) = state.client.as_ref() else {
            return Ok(RuntimeStatus {
                runtime_id: None,
                state: state.lifecycle.clone(),
                profile: None,
                capabilities: None,
                queued_messages: 0,
                in_flight_messages: 0,
                event_stream_position: 0,
                config_revision: 0,
            });
        };
        let Some(session) = state.session.as_ref() else {
            return Err(Error::not_started());
        };
        let snapshot = client.snapshot().map_err(Error::from)?;
        let session = session.lock().expect("app session mutex poisoned");
        Ok(RuntimeStatus {
            runtime_id: Some(snapshot.runtime_id.clone()),
            state: map_runtime_state(snapshot.state, session.degraded),
            profile: Some(session.config.profile.clone()),
            capabilities: Some(CapabilitySummary::from(&session.handle)),
            queued_messages: snapshot.queued_messages,
            in_flight_messages: snapshot.in_flight_messages,
            event_stream_position: snapshot.event_stream_position,
            config_revision: snapshot.config_revision,
        })
    }

    pub fn stop(&self, mode: ShutdownMode) -> Result<(), Error> {
        let mut state = self.state.lock().expect("app client mutex poisoned");
        let Some(client) = state.client.as_ref().cloned() else {
            state.session = None;
            state.lifecycle = RunState::Stopped;
            return Ok(());
        };
        let previous_lifecycle = state.lifecycle.clone();
        state.lifecycle = RunState::Stopping;
        if let Err(err) = client.shutdown(mode) {
            state.lifecycle = previous_lifecycle;
            return Err(Error::from(err));
        }
        state.client = None;
        state.session = None;
        state.lifecycle = RunState::Stopped;
        Ok(())
    }

    #[cfg(feature = "sdk-async")]
    pub fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventStream<B>, Error>
    where
        B: SdkBackendAsyncEvents,
    {
        let state = self.state.lock().expect("app client mutex poisoned");
        let Some(client) = state.client.as_ref() else {
            return Err(Error::not_started());
        };
        let Some(session) = state.session.as_ref() else {
            return Err(Error::not_started());
        };
        let subscription = client.subscribe_events(start.into()).map_err(Error::from)?;
        let session_guard = session.lock().expect("app session mutex poisoned");
        let max_batch_size = session_guard
            .config
            .event_batch_size
            .unwrap_or(session_guard.handle.effective_limits.max_poll_events)
            .min(session_guard.handle.effective_limits.max_poll_events)
            .max(1);
        let profile = session_guard.config.profile.clone();
        drop(session_guard);
        Ok(EventStream {
            client: Arc::clone(client),
            session: Arc::clone(session),
            cursor: subscription_cursor(&subscription),
            max_batch_size,
            profile,
        })
    }
}
