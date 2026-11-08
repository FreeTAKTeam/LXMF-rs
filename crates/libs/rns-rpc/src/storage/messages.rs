include!("messages_parts/module_prelude.rs");

mod announce_projection;
mod announce_retention;
mod handled_propagation;
mod message_budget;
mod unhandled_propagation;

include!("messages_parts/messagesstore.rs");

include!("messages_parts/outbound_message.rs");

#[cfg(test)]
mod broker_dispatch_tests;

#[cfg(test)]
mod retryable_outbound_tests;
