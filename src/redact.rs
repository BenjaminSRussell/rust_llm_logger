//! Secret redaction before anything is persisted (#9).
//!
//! Defaults:
//! - Headers dropped entirely from stored rows: `authorization`, `proxy-authorization`,
//!   `cookie`, `set-cookie`, `api-key`, `x-api-key`, `openai-api-key`, `anthropic-api-key`,
//!   `x-goog-api-key`, and anything containing `token` / `secret`.
//! - Stored text (prompt) has bearer tokens and common key shapes (`sk-...`, `sk-ant-...`,
//!   `AIza...`, `hf_...`) replaced with `[REDACTED]`.

use hyper::header::HeaderMap;

pub const REDACTED: &str = "[REDACTED]";

const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "api-key",
    "x-api-key",
    "openai-api-key",
    "anthropic-api-key",
    "x-goog-api-key",
];

pub fn is_sensitive_header(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    SENSITIVE_HEADERS.contains(&n.as_str()) || n.contains("token") || n.contains("secret")
}

/// Header names/values safe to persist; sensitive headers are omitted entirely.
pub fn safe_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = headers
        .iter()
        .filter(|(k, _)| !is_sensitive_header(k.as_str()))
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                redact_text(&String::from_utf8_lossy(v.as_bytes())),
            )
        })
        .collect();
    out.sort();
    out
}

fn is_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Mask secrets embedded in free text.
pub fn redact_text(input: &str) -> String {
    const PREFIXES: &[(&str, usize)] = &[("sk-", 16), ("AIza", 20), ("hf_", 16), ("xoxb-", 10)];
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // "Bearer <token>"
        let rest: String = chars[i..chars.len().min(i + 7)].iter().collect();
        if rest.eq_ignore_ascii_case("bearer ") && (i == 0 || !is_key_char(chars[i - 1])) {
            let mut j = i + 7;
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            let start = j;
            while j < chars.len() && !chars[j].is_whitespace() && chars[j] != '"' {
                j += 1;
            }
            if j > start {
                out.push_str("Bearer ");
                out.push_str(REDACTED);
                i = j;
                continue;
            }
        }
        let mut matched = false;
        if i == 0 || !is_key_char(chars[i - 1]) {
            for (prefix, min_len) in PREFIXES {
                let plen = prefix.chars().count();
                let cand: String = chars[i..chars.len().min(i + plen)].iter().collect();
                if cand == *prefix {
                    let mut j = i + plen;
                    while j < chars.len() && is_key_char(chars[j]) {
                        j += 1;
                    }
                    if j - i >= *min_len {
                        out.push_str(REDACTED);
                        i = j;
                        matched = true;
                        break;
                    }
                }
            }
        }
        if !matched {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::header::HeaderValue;

    #[test]
    fn strips_sensitive_headers() {
        let mut h = HeaderMap::new();
        h.insert("authorization", HeaderValue::from_static("Bearer sk-abc"));
        h.insert("x-api-key", HeaderValue::from_static("k"));
        h.insert("cookie", HeaderValue::from_static("session=1"));
        h.insert("x-auth-token", HeaderValue::from_static("t"));
        h.insert("content-type", HeaderValue::from_static("application/json"));
        let safe = safe_headers(&h);
        assert_eq!(
            safe,
            vec![("content-type".to_string(), "application/json".to_string())]
        );
    }

    #[test]
    fn masks_keys_in_text() {
        let s = "use key sk-proj-ABCDEFGHIJKLMNOPQRST and Bearer eyJhbGciOi.x.y please";
        let r = redact_text(s);
        assert!(!r.contains("sk-proj"), "{r}");
        assert!(!r.contains("eyJhbGciOi"), "{r}");
        assert_eq!(r, "use key [REDACTED] and Bearer [REDACTED] please");
        assert_eq!(redact_text("task-list is fine"), "task-list is fine");
        assert_eq!(redact_text("short sk-1"), "short sk-1");
    }
}
