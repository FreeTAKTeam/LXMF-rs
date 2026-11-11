use super::*;
use std::time::{Duration, Instant};

pub(super) const MAX_BROKER_SESSIONS: usize = 4096;
pub(super) const BROKER_SESSION_IDLE_TTL: Duration = Duration::from_secs(300);

impl BrokerSessionActivity {
    fn is_expired(self, now: Instant) -> bool {
        self.in_flight == 0 && now.duration_since(self.last_accepted) >= BROKER_SESSION_IDLE_TTL
    }
}

pub(in crate::rpc::daemon) enum BrokerSessionUse<'a> {
    Untracked,
    Expired,
    InFlight(BrokerSessionRequest<'a>),
}

impl BrokerSessionUse<'_> {
    pub(in crate::rpc::daemon) fn accepted_at(&mut self, now: Instant) {
        if let Self::InFlight(request) = self {
            request.accepted_at = Some(now);
        }
    }
}

/// Keep session grants available while an authenticated RPC uses them.
pub(in crate::rpc::daemon) struct BrokerSessionRequest<'a> {
    daemon: &'a RpcDaemon,
    session: &'a str,
    accepted_at: Option<Instant>,
}

impl Drop for BrokerSessionRequest<'_> {
    fn drop(&mut self) {
        let mut sessions =
            self.daemon.sdk_identity_sessions.lock().expect("identity sessions poisoned");
        let activity = sessions
            .entries
            .get_mut(self.session)
            .and_then(|session| session.broker_activity.as_mut())
            .expect("in-flight broker session cannot expire");
        activity.in_flight -= 1;
        if let Some(accepted_at) = self.accepted_at {
            activity.last_accepted = activity.last_accepted.max(accepted_at);
        }
    }
}

impl RpcDaemon {
    #[allow(clippy::result_large_err)]
    pub(super) fn admit_broker_session(
        &self,
        session: String,
        now: Instant,
    ) -> Result<(), RpcError> {
        let mut sessions = self.sdk_identity_sessions.lock().expect("identity sessions poisoned");
        if sessions.entries.get(&session).is_some_and(|entry| {
            entry.broker_activity.is_some_and(|activity| !activity.is_expired(now))
        }) {
            return Ok(());
        }
        sessions.reap_idle(now);
        if !sessions.entries.contains_key(&session) && sessions.entries.len() >= MAX_BROKER_SESSIONS
        {
            return Err(RpcError::new(
                "SDK_STORAGE_WRITE_BUSY",
                "negotiated session capacity exhausted",
            ));
        }
        sessions.entries.entry(session).or_default().broker_activity =
            Some(BrokerSessionActivity { last_accepted: now, in_flight: 0 });
        Ok(())
    }

    pub(in crate::rpc::daemon) fn begin_broker_session_request<'a>(
        &'a self,
        session: &'a str,
        now: Instant,
    ) -> BrokerSessionUse<'a> {
        let mut sessions = self.sdk_identity_sessions.lock().expect("identity sessions poisoned");
        let Some(activity) =
            sessions.entries.get_mut(session).and_then(|session| session.broker_activity.as_mut())
        else {
            return BrokerSessionUse::Untracked;
        };
        if activity.is_expired(now) {
            sessions.entries.remove(session);
            sessions.expired_total += 1;
            return BrokerSessionUse::Expired;
        }
        activity.in_flight += 1;
        BrokerSessionUse::InFlight(BrokerSessionRequest {
            daemon: self,
            session,
            accepted_at: None,
        })
    }

    pub(in crate::rpc::daemon) fn broker_session_counts(
        &self,
    ) -> Result<JsonValue, std::io::Error> {
        let mut sessions = self.sdk_identity_sessions.lock().map_err(|error| {
            std::io::Error::other(format!("resource identity sessions lock: {error}"))
        })?;
        sessions.reap_idle(Instant::now());
        let current =
            sessions.entries.values().filter(|session| session.broker_activity.is_some()).count();
        Ok(json!({
            "current": current,
            "expired_total": sessions.expired_total
        }))
    }
}

impl SdkIdentitySessions {
    pub(super) fn reap_idle(&mut self, now: Instant) {
        let before = self.entries.len();
        self.entries.retain(|_, session| {
            session.broker_activity.is_none_or(|activity| !activity.is_expired(now))
        });
        self.expired_total += before - self.entries.len();
    }
}
