use super::SdkError;
use serde_json::json;
use tokio::time::Instant;

/// Borrowed, call-local bookkeeping. Diagnostic strings allocate only on failure.
pub(super) struct ExchangeContext<'a> {
    session: &'a str,
    method: &'a str,
    pub(super) request_id: u64,
    started: Instant,
    pub(super) stage: &'static str,
    pub(super) send_completed: bool,
    pub(super) ignored_replies: u64,
}

impl<'a> ExchangeContext<'a> {
    pub(super) fn new(
        session: &'a str,
        method: &'a str,
        request_id: u64,
        started: Instant,
    ) -> Self {
        Self {
            session,
            method,
            request_id,
            started,
            stage: "request encode",
            send_completed: false,
            ignored_replies: 0,
        }
    }

    pub(super) fn timeout(&self) -> SdkError {
        SdkError::new(
            "SDK_TRANSPORT_ZMQ_TIMEOUT",
            super::ErrorCategory::Timeout,
            format!("zmq rpc request {} timed out during {}", self.request_id, self.stage),
        )
    }

    pub(super) fn annotate(&self, mut error: SdkError) -> SdkError {
        let session = diagnostic_identifier(self.session);
        let method = diagnostic_identifier(self.method);
        let elapsed_ms = self.started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        // Local send success is not daemon receipt, handler entry or delivery.
        let context = json!({
            "session_id": session,
            "request_id": self.request_id,
            "method": method,
            "stage": self.stage,
            "elapsed_ms": elapsed_ms,
            "send_completed": self.send_completed,
            "ignored_replies": self.ignored_replies,
        });
        error.message.push_str(&format!(
            " [sdk_zmq_exchange session={session} request={} method={method} stage={} elapsed_ms={elapsed_ms} send_completed={} ignored_replies={}]",
            self.request_id, self.stage, self.send_completed, self.ignored_replies,
        ));
        // Preserve all pre-existing details, including an untrusted colliding key.
        // In that case only the suffix is authoritative local context.
        error.details.entry("sdk_zmq_exchange".to_owned()).or_insert(context);
        error
    }
}

fn diagnostic_identifier(value: &str) -> String {
    value
        .chars()
        .take(128)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotation_preserves_mapped_error_fields_and_colliding_details() {
        let mut original =
            SdkError::new("original-code", super::super::ErrorCategory::Policy, "original message")
                .with_retryable(true)
                .with_user_actionable(true)
                .with_cause_code("original-cause")
                .with_detail("other", json!({"retained": true}))
                .with_detail("sdk_zmq_exchange", json!({"untrusted": "remote"}));
        original.extensions.insert("original-extension".to_owned(), json!(17));
        let mut context =
            ExchangeContext::new("session-1", "sdk_poll_events_v2", 9, Instant::now());
        context.stage = "correlated response";
        context.send_completed = true;
        context.ignored_replies = u64::MAX;
        let annotated = context.annotate(original.clone());
        assert_eq!(annotated.machine_code, original.machine_code);
        assert_eq!(annotated.category, original.category);
        assert_eq!(annotated.retryable, original.retryable);
        assert_eq!(annotated.is_user_actionable, original.is_user_actionable);
        assert_eq!(annotated.cause_code, original.cause_code);
        assert_eq!(annotated.details, original.details);
        assert_eq!(annotated.extensions, original.extensions);
        assert!(annotated.message.starts_with(&original.message));
        assert!(annotated.message.contains("method=sdk_poll_events_v2"));
        assert!(annotated.message.contains("send_completed=true"));
        assert!(annotated.message.contains(&format!("ignored_replies={}", u64::MAX)));
    }

    #[test]
    fn new_context_is_bounded_ascii_and_has_only_local_identifiers() {
        let long = format!("line\n雪/{}", "x".repeat(300));
        let context = ExchangeContext::new(&long, &long, 1, Instant::now());
        let error = context.annotate(context.timeout());
        let detail = &error.details["sdk_zmq_exchange"];
        assert_eq!(detail.as_object().expect("context object").len(), 7);
        for key in ["session_id", "method"] {
            let value = detail[key].as_str().expect("identifier");
            assert_eq!(value.len(), 128);
            assert!(value.is_ascii());
            assert!(value.starts_with("line___"));
        }
        assert!(!error.message.contains('\n'));
        assert!(!error.message.contains('雪'));
        assert_eq!(detail["send_completed"], false);
        assert_eq!(detail["ignored_replies"], 0);
    }
}
