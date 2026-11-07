include!("link_parts/module_prelude.rs");

include!("link_parts/link_mode.rs");

include!("link/payload.rs");

include!("link/id.rs");

include!("link_parts/linkhandleresult.rs");

include!("link_parts/link.rs");

include!("link_parts/clamp_link_signalling.rs");

include!("link/proof.rs");
include!("link/proof_hops.rs");

#[path = "link/request_receipt/mod.rs"]
mod request_receipt;
pub(crate) use request_receipt::{RequestCleanup, RequestDispatchGuard, RequestSource};
pub use request_receipt::{
    RequestFailure, RequestReceipt, RequestResponse, RequestSnapshot, RequestStatus,
};
include!("link/request_receipt/link_owner.rs");

#[cfg(test)]
#[path = "link/tests/request_receipts.rs"]
mod request_receipt_tests;

include!("link_parts/link_status_predicates_make_lifecycl.rs");
