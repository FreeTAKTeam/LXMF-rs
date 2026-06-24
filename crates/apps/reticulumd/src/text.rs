//! Shared UTF-8 decode helpers.
//!
//! These return `Result` so callers can distinguish "no value" from "invalid
//! UTF-8". A warning is logged at the point of failure (carrying `context`)
//! while the conversion work to propagate the error through callers is in
//! progress; once every caller propagates, the logging here can be dropped.

/// Decode a byte slice as UTF-8, logging (with `context`) on failure.
pub fn decode_utf8<'a>(data: &'a [u8], context: &str) -> Result<&'a str, std::str::Utf8Error> {
    std::str::from_utf8(data)
        .inspect_err(|err| log::warn!("[daemon] invalid UTF-8 in {context}: {err}"))
}

/// Decode owned bytes as a UTF-8 `String`, logging (with `context`) on failure.
pub fn decode_utf8_owned(
    data: Vec<u8>,
    context: &str,
) -> Result<String, std::string::FromUtf8Error> {
    String::from_utf8(data)
        .inspect_err(|err| log::warn!("[daemon] invalid UTF-8 in {context}: {err}"))
}
