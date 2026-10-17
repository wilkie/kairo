//! The parts of Git's protocol v2 the federation server answers or
//! filters itself (`specs/DECISIONS.md` §22).
//!
//! `git upload-pack` in protocol v2 serves any object it can read,
//! regardless of `uploadpack.allow*SHA1InWant`, and a per-object repo
//! reads the whole shared pool through its alternates. So the server
//! never hands a peer's request to it unchecked:
//!
//! - **`ls-refs` is answered here**, advertising one ref per published
//!   commit (`refs/kairo/revisions/<oid>`), so a peer only ever learns
//!   of published commits.
//! - **`fetch` is filtered here.** Every `want` must be a published
//!   commit, or the request is refused. A `have` that isn't one is
//!   dropped, so `upload-pack` can neither acknowledge an unpublished
//!   object (an existence oracle) nor delta against it. `want-ref` and
//!   `packfile-uris` are refused.
//! - **Every other command is refused**, and the capability
//!   advertisement is rewritten to the capabilities above.
//!
//! The format, from Git's `gitprotocol-v2`: a request is one command
//! as pkt-lines (`command=<name>`, capability lines, a delimiter
//! `0001`, argument lines, a flush `0000`). Each pkt-line is a 4-hex
//! length that counts itself, then the payload.

use std::collections::BTreeSet;

/// Largest pkt-line, length prefix included (`LARGE_PACKET_MAX`).
const MAX_PKT_LEN: usize = 65520;

/// Capabilities passed through from `upload-pack`'s advertisement.
/// Everything else (`object-info`, `bundle-uri`, …) is dropped.
const ADVERTISED: &[&str] = &[
    "version",
    "agent",
    "fetch",
    "object-format",
    "server-option",
];

/// Ref namespace the server advertises published commits under.
pub(crate) const REVISION_REF_PREFIX: &str = "refs/kairo/revisions/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pkt<'a> {
    Flush,
    Delim,
    ResponseEnd,
    Data(&'a [u8]),
}

/// Why a request was refused. The message is shown to the peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refusal(pub String);

fn refuse(message: impl Into<String>) -> Refusal {
    Refusal(message.into())
}

/// Split `buf` into pkt-lines. The whole buffer must be pkt-lines.
pub(crate) fn parse_pkts(mut buf: &[u8]) -> Result<Vec<Pkt<'_>>, Refusal> {
    let mut pkts = Vec::new();
    while !buf.is_empty() {
        let header = buf
            .get(..4)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| usize::from_str_radix(h, 16).ok())
            .ok_or_else(|| refuse("malformed pkt-line length"))?;
        match header {
            0 => pkts.push(Pkt::Flush),
            1 => pkts.push(Pkt::Delim),
            2 => pkts.push(Pkt::ResponseEnd),
            3 => return Err(refuse("malformed pkt-line length")),
            len if len > MAX_PKT_LEN => return Err(refuse("pkt-line too long")),
            len => {
                let data = buf
                    .get(4..len)
                    .ok_or_else(|| refuse("truncated pkt-line"))?;
                pkts.push(Pkt::Data(data));
                buf = &buf[len..];
                continue;
            }
        }
        buf = &buf[4..];
    }
    Ok(pkts)
}

/// Append `data` as one pkt-line.
pub(crate) fn write_pkt(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(format!("{:04x}", data.len() + 4).as_bytes());
    out.extend_from_slice(data);
}

fn write_flush(out: &mut Vec<u8>) {
    out.extend_from_slice(b"0000");
}

fn line(data: &[u8]) -> &[u8] {
    data.strip_suffix(b"\n").unwrap_or(data)
}

/// One v2 command request.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Request<'a> {
    pub command: &'a str,
    /// Capability lines after the command, as sent.
    pub capabilities: Vec<&'a [u8]>,
    /// Argument lines after the delimiter, as sent.
    pub args: Vec<&'a [u8]>,
}

/// Parse a stateless request body: exactly one command.
pub(crate) fn parse_request(body: &[u8]) -> Result<Request<'_>, Refusal> {
    let pkts = parse_pkts(body)?;
    let mut pkts = pkts.into_iter();
    let command = match pkts.next() {
        Some(Pkt::Data(data)) => std::str::from_utf8(line(data))
            .ok()
            .and_then(|l| l.strip_prefix("command="))
            .ok_or_else(|| refuse("request does not start with a command"))?,
        _ => return Err(refuse("request does not start with a command")),
    };
    let mut request = Request {
        command,
        capabilities: Vec::new(),
        args: Vec::new(),
    };
    let mut in_args = false;
    for pkt in pkts.by_ref() {
        match pkt {
            Pkt::Data(data) if in_args => request.args.push(data),
            Pkt::Data(data) => request.capabilities.push(data),
            Pkt::Delim if !in_args => in_args = true,
            Pkt::Flush => {
                return if pkts.next().is_none() {
                    Ok(request)
                } else {
                    Err(refuse("one command per request"))
                };
            }
            _ => return Err(refuse("unexpected pkt-line in request")),
        }
    }
    Err(refuse("request not terminated by a flush"))
}

/// A 40- or 64-character lowercase hex object id.
fn is_oid(raw: &str) -> bool {
    (raw.len() == 40 || raw.len() == 64)
        && raw
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `fetch`, re-encoded for `upload-pack`: wants checked against
/// `published`, foreign haves dropped, everything else as sent.
pub(crate) fn filter_fetch(
    request: &Request<'_>,
    published: &BTreeSet<String>,
) -> Result<Vec<u8>, Refusal> {
    let mut out = Vec::new();
    write_pkt(&mut out, b"command=fetch\n");
    for capability in &request.capabilities {
        write_pkt(&mut out, capability);
    }
    out.extend_from_slice(b"0001");
    let mut wants = 0usize;
    for arg in &request.args {
        let text = std::str::from_utf8(line(arg)).map_err(|_| refuse("non-UTF-8 argument"))?;
        let (name, value) = text.split_once(' ').unwrap_or((text, ""));
        match name {
            "want" => {
                if !is_oid(value) || !published.contains(value) {
                    return Err(refuse(format!("not a published commit: {value}")));
                }
                wants += 1;
            }
            "have" => {
                if !published.contains(value) {
                    continue;
                }
            }
            "want-ref" | "packfile-uris" => {
                return Err(refuse(format!("{name} is not supported")));
            }
            _ => {}
        }
        write_pkt(&mut out, arg);
    }
    if wants == 0 {
        return Err(refuse("fetch names no wanted commit"));
    }
    write_flush(&mut out);
    Ok(out)
}

/// The `ls-refs` response: one `refs/kairo/revisions/<oid>` per
/// published commit, narrowed by any `ref-prefix` arguments. No
/// `HEAD`, no symrefs, no tags, so `peel`, `symrefs`, and `unborn`
/// have nothing to add.
pub(crate) fn ls_refs(request: &Request<'_>, published: &BTreeSet<String>) -> Vec<u8> {
    let prefixes: Vec<&[u8]> = request
        .args
        .iter()
        .filter_map(|arg| line(arg).strip_prefix(b"ref-prefix "))
        .collect();
    let mut out = Vec::new();
    for oid in published {
        let name = format!("{REVISION_REF_PREFIX}{oid}");
        if prefixes.is_empty()
            || prefixes
                .iter()
                .any(|prefix| name.as_bytes().starts_with(prefix))
        {
            write_pkt(&mut out, format!("{oid} {name}\n").as_bytes());
        }
    }
    write_flush(&mut out);
    out
}

/// `upload-pack`'s v2 capability advertisement narrowed to what this
/// server answers: [`ADVERTISED`] as given, plus `ls-refs` without
/// arguments, since it's answered here.
pub(crate) fn filter_advertisement(raw: &[u8]) -> Result<Vec<u8>, Refusal> {
    let mut out = Vec::new();
    for pkt in parse_pkts(raw)? {
        match pkt {
            Pkt::Data(data) => {
                let text = line(data);
                let name = text
                    .split(|&b| b == b'=' || b == b' ')
                    .next()
                    .unwrap_or_default();
                if name == b"ls-refs" {
                    write_pkt(&mut out, b"ls-refs\n");
                } else if ADVERTISED.iter().any(|allowed| name == allowed.as_bytes()) {
                    write_pkt(&mut out, data);
                }
            }
            Pkt::Flush => write_flush(&mut out),
            _ => return Err(refuse("unexpected pkt-line in advertisement")),
        }
    }
    Ok(out)
}

/// Whether a `Git-Protocol` header value asks for protocol v2. The
/// value is colon-separated `key=value` parameters.
pub(crate) fn wants_v2(header: &str) -> bool {
    header.split(':').any(|param| param.trim() == "version=2")
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const PUBLISHED: &str = "1111111111111111111111111111111111111111";
    const SECRET: &str = "2222222222222222222222222222222222222222";

    fn published() -> BTreeSet<String> {
        [PUBLISHED.to_owned()].into()
    }

    fn request(command: &str, args: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        write_pkt(&mut out, format!("command={command}\n").as_bytes());
        write_pkt(&mut out, b"agent=git/2.43.0\n");
        out.extend_from_slice(b"0001");
        for arg in args {
            write_pkt(&mut out, format!("{arg}\n").as_bytes());
        }
        out.extend_from_slice(b"0000");
        out
    }

    fn data_lines(buf: &[u8]) -> Vec<String> {
        parse_pkts(buf)
            .expect("pkts")
            .into_iter()
            .filter_map(|pkt| match pkt {
                Pkt::Data(data) => Some(String::from_utf8_lossy(line(data)).into_owned()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn pkt_lines_round_trip() {
        let mut out = Vec::new();
        write_pkt(&mut out, b"hello\n");
        out.extend_from_slice(b"00010002");
        write_flush(&mut out);
        assert_eq!(&out[..10], b"000ahello\n");
        assert_eq!(
            parse_pkts(&out).expect("parse"),
            vec![
                Pkt::Data(b"hello\n"),
                Pkt::Delim,
                Pkt::ResponseEnd,
                Pkt::Flush
            ]
        );
        for bad in [&b"00"[..], b"zzzz", b"0003", b"0009abc", b"fff1"] {
            assert!(parse_pkts(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_request_is_one_flush_terminated_command() {
        let body = request("fetch", &["done"]);
        let parsed = parse_request(&body).expect("parse");
        assert_eq!(parsed.command, "fetch");
        assert_eq!(parsed.capabilities, vec![&b"agent=git/2.43.0\n"[..]]);
        assert_eq!(parsed.args, vec![&b"done\n"[..]]);

        let mut two = body.clone();
        two.extend_from_slice(&body);
        assert!(parse_request(&two).is_err());
        assert!(parse_request(&body[..body.len() - 4]).is_err());
    }

    #[test]
    fn fetch_refuses_unpublished_wants() {
        let body = request("fetch", &[&format!("want {SECRET}"), "done"]);
        let refusal =
            filter_fetch(&parse_request(&body).expect("parse"), &published()).expect_err("refused");
        assert!(refusal.0.contains(SECRET));

        for unsupported in ["want-ref refs/heads/main", "packfile-uris https"] {
            let body = request("fetch", &[&format!("want {PUBLISHED}"), unsupported]);
            assert!(filter_fetch(&parse_request(&body).expect("parse"), &published()).is_err());
        }
        let body = request("fetch", &["done"]);
        assert!(filter_fetch(&parse_request(&body).expect("parse"), &published()).is_err());
    }

    #[test]
    fn fetch_drops_unpublished_haves_and_keeps_the_rest() {
        let body = request(
            "fetch",
            &[
                "thin-pack",
                &format!("want {PUBLISHED}"),
                &format!("have {SECRET}"),
                &format!("have {PUBLISHED}"),
                "done",
            ],
        );
        let filtered =
            filter_fetch(&parse_request(&body).expect("parse"), &published()).expect("filtered");
        assert_eq!(
            data_lines(&filtered),
            vec![
                "command=fetch".to_owned(),
                "agent=git/2.43.0".to_owned(),
                "thin-pack".to_owned(),
                format!("want {PUBLISHED}"),
                format!("have {PUBLISHED}"),
                "done".to_owned(),
            ]
        );
        assert!(filtered.ends_with(b"0000"));
    }

    #[test]
    fn ls_refs_lists_published_commits_by_prefix() {
        let body = request("ls-refs", &["peel", "symrefs"]);
        let all = ls_refs(&parse_request(&body).expect("parse"), &published());
        assert_eq!(
            data_lines(&all),
            vec![format!("{PUBLISHED} refs/kairo/revisions/{PUBLISHED}")]
        );
        let body = request("ls-refs", &["ref-prefix refs/heads/"]);
        let none = ls_refs(&parse_request(&body).expect("parse"), &published());
        assert_eq!(none, b"0000");
    }

    #[test]
    fn the_advertisement_keeps_only_answered_capabilities() {
        let mut raw = Vec::new();
        for cap in [
            "version 2",
            "agent=git/2.43.0",
            "ls-refs=unborn",
            "fetch=shallow wait-for-done filter",
            "server-option",
            "object-format=sha1",
            "object-info",
            "bundle-uri",
        ] {
            write_pkt(&mut raw, format!("{cap}\n").as_bytes());
        }
        raw.extend_from_slice(b"0000");
        let filtered = filter_advertisement(&raw).expect("filter");
        assert_eq!(
            data_lines(&filtered),
            vec![
                "version 2",
                "agent=git/2.43.0",
                "ls-refs",
                "fetch=shallow wait-for-done filter",
                "server-option",
                "object-format=sha1",
            ]
        );
    }

    #[test]
    fn git_protocol_header_parsing() {
        assert!(wants_v2("version=2"));
        assert!(wants_v2("object-format=sha256:version=2"));
        assert!(!wants_v2("version=1"));
        assert!(!wants_v2(""));
    }
}
