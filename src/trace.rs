//! W3C Trace Context (`traceparent`) propagation (#11).
//!
//! If the client sends a valid `traceparent`, its trace id is recorded and the header
//! is forwarded unchanged. Otherwise the proxy starts a new trace, forwards the
//! generated header upstream, and records that trace id.

use hyper::header::{HeaderMap, HeaderValue};
use rand::RngCore;

pub const TRACEPARENT: &str = "traceparent";

fn is_lower_hex(s: &str) -> bool {
    s.bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Parse `version-traceid-parentid-flags`; returns the 32-hex trace id if valid.
pub fn parse_traceparent(value: &str) -> Option<String> {
    let parts: Vec<&str> = value.trim().split('-').collect();
    if parts.len() < 4 {
        return None;
    }
    let (ver, trace, parent, flags) = (parts[0], parts[1], parts[2], parts[3]);
    let ok = ver.len() == 2
        && ver != "ff"
        && trace.len() == 32
        && parent.len() == 16
        && flags.len() == 2
        && [ver, trace, parent, flags].iter().all(|p| is_lower_hex(p))
        && trace != "0".repeat(32)
        && parent != "0".repeat(16);
    ok.then(|| trace.to_string())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Generate a fresh sampled `traceparent`, returning (header, trace_id).
pub fn new_traceparent() -> (String, String) {
    let mut rng = rand::thread_rng();
    let mut trace = [0u8; 16];
    let mut span = [0u8; 8];
    loop {
        rng.fill_bytes(&mut trace);
        rng.fill_bytes(&mut span);
        if trace.iter().any(|b| *b != 0) && span.iter().any(|b| *b != 0) {
            break;
        }
    }
    let trace_id = hex(&trace);
    (format!("00-{}-{}-01", trace_id, hex(&span)), trace_id)
}

/// Ensure `headers` carries a valid `traceparent`; returns the trace id.
pub fn ensure_traceparent(headers: &mut HeaderMap) -> String {
    if let Some(id) = headers
        .get(TRACEPARENT)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_traceparent)
    {
        return id;
    }
    let (header, id) = new_traceparent();
    if let Ok(v) = HeaderValue::from_str(&header) {
        headers.insert(TRACEPARENT, v);
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_and_rejects_invalid() {
        let tp = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        assert_eq!(
            parse_traceparent(tp).as_deref(),
            Some("4bf92f3577b34da6a3ce929d0e0e4736")
        );
        assert!(
            parse_traceparent("00-0000000000000000000000000000000-00f067aa0ba902b7-01").is_none()
        );
        assert!(
            parse_traceparent("00-00000000000000000000000000000000-00f067aa0ba902b7-01").is_none()
        );
        assert!(parse_traceparent("garbage").is_none());
        assert!(
            parse_traceparent("00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01").is_none()
        );
    }

    #[test]
    fn existing_header_is_kept() {
        let mut h = HeaderMap::new();
        let tp = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        h.insert(TRACEPARENT, HeaderValue::from_static(tp));
        assert_eq!(
            ensure_traceparent(&mut h),
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
        assert_eq!(h.get(TRACEPARENT).unwrap(), tp);
    }

    #[test]
    fn missing_header_creates_new_trace() {
        let mut h = HeaderMap::new();
        let id = ensure_traceparent(&mut h);
        let sent = h.get(TRACEPARENT).unwrap().to_str().unwrap();
        assert_eq!(parse_traceparent(sent).as_deref(), Some(id.as_str()));
        let mut h2 = HeaderMap::new();
        assert_ne!(ensure_traceparent(&mut h2), id);
    }
}
