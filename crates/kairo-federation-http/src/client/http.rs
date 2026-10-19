//! Blocking HTTP transport for pulls.
//!
//! One `ureq` agent per pull, with the §2.4 limits applied to every
//! request: a global per-request timeout, a body-size cap chosen per
//! call, no redirects (a redirect could hop from `https://` to a
//! plain-`http://` host the URL check would have refused), and non-2xx
//! statuses returned rather than raised, so the federation error
//! envelope can be surfaced.

use kairo_federation_core::FedErrorEnvelope;
use serde::de::DeserializeOwned;
use ureq::Agent;

use super::{Limits, PullError};

#[derive(Debug)]
pub(crate) struct Peer {
    agent: Agent,
    base_url: String,
}

impl Peer {
    pub(crate) fn new(base_url: &str, limits: &Limits) -> Result<Self, PullError> {
        let base_url = normalize_base_url(base_url)?;
        let agent: Agent = Agent::config_builder()
            .timeout_global(Some(limits.request_timeout))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .into();
        Ok(Self { agent, base_url })
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    /// `GET <base><path>`, returning at most `limit` body bytes.
    pub(crate) fn get_bytes(&self, path: &str, limit: u64) -> Result<Vec<u8>, PullError> {
        let url = format!("{}{path}", self.base_url);
        let response = self
            .agent
            .get(&url)
            .call()
            .map_err(|error| PullError::Transport {
                url: url.clone(),
                message: error.to_string(),
            })?;
        Self::read_body(url, response, limit)
    }

    /// `POST <base><path>` with a JSON `body`, returning at most `limit`
    /// response bytes. Only for endpoints that read (`DECISIONS.md`
    /// §24).
    pub(crate) fn post_json<T: serde::Serialize>(
        &self,
        path: &str,
        body: &T,
        limit: u64,
    ) -> Result<Vec<u8>, PullError> {
        let url = format!("{}{path}", self.base_url);
        let bytes = serde_json::to_vec(body).map_err(|error| PullError::Parse {
            url: url.clone(),
            message: error.to_string(),
        })?;
        let response = self
            .agent
            .post(&url)
            .header("content-type", "application/json")
            .send(&bytes[..])
            .map_err(|error| PullError::Transport {
                url: url.clone(),
                message: error.to_string(),
            })?;
        Self::read_body(url, response, limit)
    }

    /// `POST <base><path>` with a JSON `body` and an `Authorization`
    /// header, for endpoints that act on who is asking (`DECISIONS.md`
    /// §25). `body` is sent exactly as signed.
    pub(crate) fn post_signed(
        &self,
        path: &str,
        body: &[u8],
        authorization: &str,
        limit: u64,
    ) -> Result<Vec<u8>, PullError> {
        let url = format!("{}{path}", self.base_url);
        let response = self
            .agent
            .post(&url)
            .header("content-type", "application/json")
            .header("authorization", authorization)
            .send(body)
            .map_err(|error| PullError::Transport {
                url: url.clone(),
                message: error.to_string(),
            })?;
        Self::read_body(url, response, limit)
    }

    /// [`Peer::post_signed`] with a file of `length` bytes as the body,
    /// streamed rather than read into memory (an upload, §26).
    pub(crate) fn post_signed_file(
        &self,
        path: &str,
        mut file: std::fs::File,
        length: u64,
        authorization: &str,
        limit: u64,
    ) -> Result<Vec<u8>, PullError> {
        let url = format!("{}{path}", self.base_url);
        let response = self
            .agent
            .post(&url)
            .header("content-type", "application/x-tar")
            .header("content-length", length.to_string())
            .header("authorization", authorization)
            .send(ureq::SendBody::from_reader(&mut file))
            .map_err(|error| PullError::Transport {
                url: url.clone(),
                message: error.to_string(),
            })?;
        Self::read_body(url, response, limit)
    }

    /// A response's body, at most `limit` bytes for a 2xx; a non-2xx
    /// becomes [`PullError::Status`] with the envelope's code.
    fn read_body(
        url: String,
        mut response: ureq::http::Response<ureq::Body>,
        limit: u64,
    ) -> Result<Vec<u8>, PullError> {
        let status = response.status().as_u16();
        // Error bodies are small envelopes; cap them tighter than data.
        let read_limit = if (200..300).contains(&status) {
            limit
        } else {
            limit.min(64 * 1024)
        };
        let body = response
            .body_mut()
            .with_config()
            .limit(read_limit)
            .read_to_vec()
            .map_err(|error| match error {
                ureq::Error::BodyExceedsLimit(_) => PullError::TooLarge {
                    url: url.clone(),
                    limit: read_limit,
                },
                other => PullError::Transport {
                    url: url.clone(),
                    message: other.to_string(),
                },
            })?;
        if (200..300).contains(&status) {
            return Ok(body);
        }
        let envelope = serde_json::from_slice::<FedErrorEnvelope>(&body).ok();
        Err(PullError::Status {
            url,
            status,
            code: envelope.as_ref().map(|e| e.error.code),
            message: envelope.map(|e| e.error.message),
        })
    }

    /// [`Peer::get_bytes`] parsed as JSON.
    pub(crate) fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        limit: u64,
    ) -> Result<T, PullError> {
        let bytes = self.get_bytes(path, limit)?;
        serde_json::from_slice(&bytes).map_err(|error| PullError::Parse {
            url: format!("{}{path}", self.base_url),
            message: error.to_string(),
        })
    }
}

/// Accept `https://<host>` for any host, or `http://<host>` only when
/// the host is loopback (tests, and a client on the serving machine).
/// A trailing `/` is dropped; any other path, query, or fragment is
/// refused, since endpoint paths are joined onto the result.
pub(crate) fn normalize_base_url(raw: &str) -> Result<String, PullError> {
    let refuse = |reason: &str| PullError::InvalidPeerUrl {
        url: raw.to_owned(),
        reason: reason.to_owned(),
    };
    let trimmed = raw.strip_suffix('/').unwrap_or(raw);
    let lower = trimmed.to_ascii_lowercase();
    let (secure, authority) = if lower.starts_with("https://") {
        (true, &trimmed["https://".len()..])
    } else if lower.starts_with("http://") {
        (false, &trimmed["http://".len()..])
    } else {
        return Err(refuse(
            "must start with https:// (or http:// for a loopback host)",
        ));
    };
    if authority.is_empty()
        || authority
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '/' | '?' | '#' | '@'))
    {
        return Err(refuse(
            "must be an origin with no path, query, or credentials",
        ));
    }
    if !secure && !is_loopback_authority(authority) {
        return Err(refuse("plain http:// is only allowed for loopback hosts"));
    }
    // Scheme and host are case-insensitive; lowercase both so a
    // config entry and `--from` compare equal however they're typed.
    let scheme = if secure { "https" } else { "http" };
    Ok(format!("{scheme}://{}", authority.to_ascii_lowercase()))
}

fn is_loopback_authority(authority: &str) -> bool {
    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::normalize_base_url;

    #[test]
    fn accepts_https_anywhere_and_http_on_loopback() {
        for (raw, normalized) in [
            ("https://archive.example.org", "https://archive.example.org"),
            (
                "https://archive.example.org/",
                "https://archive.example.org",
            ),
            (
                "https://archive.example.org:8443",
                "https://archive.example.org:8443",
            ),
            ("http://127.0.0.1:7880", "http://127.0.0.1:7880"),
            ("http://localhost:7880/", "http://localhost:7880"),
            ("http://[::1]:7880", "http://[::1]:7880"),
            (
                "HTTPS://Archive.Example.ORG/",
                "https://archive.example.org",
            ),
        ] {
            assert_eq!(
                normalize_base_url(raw).ok().as_deref(),
                Some(normalized),
                "{raw}"
            );
        }
    }

    #[test]
    fn refuses_plain_http_off_loopback_and_odd_shapes() {
        for raw in [
            "http://archive.example.org",
            "http://10.0.0.5:7880",
            "http://localhost.evil.example",
            "ftp://archive.example.org",
            "archive.example.org",
            "https://",
            "https://archive.example.org/kairo",
            "https://user@archive.example.org",
            "https://archive.example.org?x=1",
        ] {
            assert!(normalize_base_url(raw).is_err(), "{raw} accepted");
        }
    }
}
