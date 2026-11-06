//! Shared browser-origin and DNS-rebinding protection for every MeshChat route.

use axum::{
    extract::{Request, State},
    http::{header, uri::Authority, HeaderMap, HeaderName, StatusCode, Version},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::net::{IpAddr, SocketAddr};

#[derive(Debug)]
struct InvalidRequest;

type PolicyResult<T> = Result<T, InvalidRequest>;

#[derive(Clone, Copy)]
pub(super) struct RequestPolicy {
    bind: Option<SocketAddr>,
}

impl RequestPolicy {
    // The public router has no listener: still forbid DNS names other than localhost.
    pub(super) fn loopback() -> Self {
        Self { bind: None }
    }

    pub(super) fn for_bind(bind: SocketAddr) -> Self {
        Self { bind: Some(bind) }
    }

    fn check(&self, request: &Request) -> PolicyResult<()> {
        let headers = request.headers();
        let host = match single_header(headers, header::HOST)? {
            Some(host) => host,
            None if request.version() == Version::HTTP_2 => {
                request.uri().authority().ok_or(InvalidRequest)?.as_str()
            }
            None => return Err(InvalidRequest),
        };
        let authority = loopback_authority(host)?;
        if let Some(bind) = self.bind {
            if !bind.ip().is_loopback()
                || authority.1 != bind.port()
                || (authority.0 != "localhost" && authority.0 != bind.ip().to_string())
            {
                return Err(InvalidRequest);
            }
        }

        // Never trust Forwarded/X-Forwarded-* as authority. An absolute request target
        // must agree with Host too; this API serves plain HTTP directly on loopback.
        if let Some(uri_authority) = request.uri().authority() {
            if request.uri().scheme_str() != Some("http")
                || loopback_authority(uri_authority.as_str())? != authority
            {
                return Err(InvalidRequest);
            }
        } else if request.uri().scheme().is_some() {
            return Err(InvalidRequest);
        }

        if let Some(origin) = single_header(headers, header::ORIGIN)? {
            // A serialized origin is only a scheme and authority, with no userinfo,
            // path, query, fragment, list, or opaque "null" origin.
            if loopback_authority(origin.strip_prefix("http://").ok_or(InvalidRequest)?)?
                != authority
            {
                return Err(InvalidRequest);
            }
        }
        if let Some(site) = single_header(headers, HeaderName::from_static("sec-fetch-site"))? {
            // Images and navigations can omit Origin even for state-changing GETs.
            // "same-site" is insufficient: separate ports are separate origins.
            if !matches!(site, "same-origin" | "none") {
                return Err(InvalidRequest);
            }
        }
        Ok(())
    }
}

// Distinguish an absent header from duplicate, non-UTF-8, or otherwise malformed
// input. HeaderMap::get alone would silently accept the first duplicate value.
fn single_header(headers: &HeaderMap, name: HeaderName) -> PolicyResult<Option<&str>> {
    let mut values = headers.get_all(name).iter();
    let value =
        values.next().map(|value| value.to_str()).transpose().map_err(|_| InvalidRequest)?;
    if values.next().is_some() {
        return Err(InvalidRequest);
    }
    Ok(value)
}

fn loopback_authority(value: &str) -> PolicyResult<(String, u16)> {
    let authority = value.parse::<Authority>().map_err(|_| InvalidRequest)?;
    // Authority accepts userinfo, so explicitly reject it instead of using host()
    // alone. Restrict syntax to literal IPs/localhost with an optional numeric port.
    if value.contains('@') {
        return Err(InvalidRequest);
    }
    let host = authority.host();
    let port = match value.strip_prefix(host).ok_or(InvalidRequest)? {
        "" => 80,
        suffix => {
            let port = suffix.strip_prefix(':').ok_or(InvalidRequest)?;
            if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(InvalidRequest);
            }
            port.parse::<u16>().map_err(|_| InvalidRequest)?
        }
    };
    let host = if host.eq_ignore_ascii_case("localhost") {
        "localhost".to_owned()
    } else {
        let literal =
            host.strip_prefix('[').and_then(|host| host.strip_suffix(']')).unwrap_or(host);
        let ip = literal.parse::<IpAddr>().map_err(|_| InvalidRequest)?;
        if !ip.is_loopback()
            || !matches!(
                (ip, host.starts_with('[')),
                (IpAddr::V4(_), false) | (IpAddr::V6(_), true)
            )
        {
            return Err(InvalidRequest);
        }
        ip.to_string()
    };
    Ok((host, port))
}

pub(super) async fn validate(
    State(policy): State<RequestPolicy>,
    request: Request,
    next: Next,
) -> Response {
    if policy.check(&request).is_err() {
        return super::ApiError(
            StatusCode::FORBIDDEN,
            "MeshChat requires a local same-origin request".into(),
        )
        .into_response();
    }
    next.run(request).await
}
