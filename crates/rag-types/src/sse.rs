//! Server-Sent Events reader for clients. Bytes arrive in arbitrary chunks (even in the middle of
//! a multi-byte character), so only complete lines are decoded.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// The `event:` name, if the server sent one.
    pub event: Option<String>,
    pub data: String,
}

#[derive(Default)]
pub struct SseReader {
    buf: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
}

impl SseReader {
    /// Feed raw bytes; returns every event completed by them. Comments, `id:` and `retry:` are ignored.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(pos) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if !self.data.is_empty() {
                    out.push(SseEvent { event: self.event.take(), data: self.data.join("\n") });
                    self.data.clear();
                }
                self.event = None;
            } else if let Some(rest) = line.strip_prefix("event:") {
                self.event = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("data:") {
                self.data.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_events_comments_and_multiline_data() {
        let mut r = SseReader::default();
        let got = r.push(b": hello\n\nevent: token\ndata: {\"text\":\"a\"}\n\nevent: done\ndata: line1\ndata: line2\n\n");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], SseEvent { event: Some("token".into()), data: "{\"text\":\"a\"}".into() });
        assert_eq!(got[1].data, "line1\nline2");
    }

    #[test]
    fn any_split_point_gives_the_same_events() {
        let stream = "event: token\r\ndata: {\"text\":\"é 漢\"}\r\n\r\nevent: done\r\ndata: {}\r\n\r\n".as_bytes();
        for cut in 1..stream.len() {
            let mut r = SseReader::default();
            let mut got = r.push(&stream[..cut]);
            got.extend(r.push(&stream[cut..]));
            assert_eq!(got.len(), 2, "cut at {cut}");
            assert_eq!(got[0].data, "{\"text\":\"é 漢\"}");
            assert_eq!(got[1].event.as_deref(), Some("done"));
        }
    }

    #[test]
    fn an_incomplete_event_waits() {
        let mut r = SseReader::default();
        assert!(r.push(b"event: token\ndata: x\n").is_empty());
        assert_eq!(r.push(b"\n").len(), 1);
    }
}
