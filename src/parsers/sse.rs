//! Byte-level Server-Sent Events framing shared by the OpenAI and Anthropic parsers.
//!
//! Events are split on raw byte offsets (never on a lossy UTF-8 copy), so a chunk
//! boundary in the middle of a multi-byte character cannot shift the cut point.
//! Accepts `\n\n`, `\r\n\r\n` and `\r\r` delimiters and `data:` with or without a space.

use bytes::BytesMut;

#[derive(Default)]
pub struct SseFramer {
    buffer: BytesMut,
}

/// Find the first event delimiter; returns (start, delimiter_len).
fn find_delimiter(buf: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i < buf.len() {
        match buf[i] {
            b'\n' if buf.get(i + 1) == Some(&b'\n') => return Some((i, 2)),
            b'\r'
                if buf.get(i + 1) == Some(&b'\n')
                    && buf.get(i + 2) == Some(&b'\r')
                    && buf.get(i + 3) == Some(&b'\n') =>
            {
                return Some((i, 4))
            }
            b'\r' if buf.get(i + 1) == Some(&b'\r') => return Some((i, 2)),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Extract the joined `data:` payload of one event block.
fn event_data(block: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(block).ok()?;
    let mut data: Vec<&str> = Vec::new();
    for line in text.split(['\n', '\r']) {
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            data.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }
    if data.is_empty() {
        None
    } else {
        Some(data.join("\n"))
    }
}

impl SseFramer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append bytes and return the `data` payloads of every complete event.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(chunk);
        let mut out = Vec::new();
        // Only rescan from where the previous scan could have stopped.
        while let Some((pos, len)) = find_delimiter(&self.buffer) {
            let block = self.buffer.split_to(pos + len);
            if let Some(d) = event_data(&block[..pos]) {
                out.push(d);
            }
        }
        out
    }

    /// Flush a trailing event that was not terminated by a blank line.
    pub fn finish(&mut self) -> Vec<String> {
        let rest = self.buffer.split();
        event_data(&rest).into_iter().collect()
    }
}
