/// Synthetic Primary Device Attributes (DA1) reply claiming VT220 with ANSI color.
pub const DA1_RESPONSE: &[u8] = b"\x1b[?62;22c";
/// Synthetic Secondary Device Attributes (DA2) reply for detached sessions.
pub const DA2_RESPONSE: &[u8] = b"\x1b[>1;10;0c";

const DA1_QUERY: &[u8] = b"\x1b[c";
const DA1_QUERY_EXPLICIT: &[u8] = b"\x1b[0c";
const DA2_QUERY: &[u8] = b"\x1b[>c";
const DA2_QUERY_EXPLICIT: &[u8] = b"\x1b[>0c";
const MAX_QUERY_LEN: usize = 4;

pub fn device_attribute_replies(data: &[u8]) -> Vec<Vec<u8>> {
    scan_device_attribute_replies(data, 0)
}

#[derive(Default)]
pub struct DeviceAttributeTracker {
    tail: Vec<u8>,
}

impl DeviceAttributeTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        let mut combined = Vec::with_capacity(self.tail.len() + data.len());
        combined.extend_from_slice(&self.tail);
        combined.extend_from_slice(data);

        let replies = scan_device_attribute_replies(&combined, self.tail.len());

        let tail_len = (MAX_QUERY_LEN - 1).min(combined.len());
        self.tail.clear();
        self.tail.extend_from_slice(&combined[combined.len() - tail_len..]);

        replies
    }
}

fn scan_device_attribute_replies(data: &[u8], min_match_end: usize) -> Vec<Vec<u8>> {
    let mut replies = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\x1b' && i + 1 < data.len() && data[i + 1] == b'[' {
            if i + 2 < data.len() && data[i + 2] == b'?' {
                i += 3;
                continue;
            }
            if let Some((reply, len)) = detect_query(&data[i..]) {
                if i + len > min_match_end {
                    replies.push(reply.to_vec());
                }
            }
        }
        i += 1;
    }
    replies
}

fn detect_query(data: &[u8]) -> Option<(&'static [u8], usize)> {
    if matches_at(data, DA2_QUERY_EXPLICIT) {
        Some((DA2_RESPONSE, DA2_QUERY_EXPLICIT.len()))
    } else if matches_at(data, DA2_QUERY) {
        Some((DA2_RESPONSE, DA2_QUERY.len()))
    } else if matches_at(data, DA1_QUERY_EXPLICIT) {
        Some((DA1_RESPONSE, DA1_QUERY_EXPLICIT.len()))
    } else if matches_at(data, DA1_QUERY) {
        Some((DA1_RESPONSE, DA1_QUERY.len()))
    } else {
        None
    }
}

fn matches_at(data: &[u8], needle: &[u8]) -> bool {
    data.starts_with(needle)
}

#[cfg(test)]
mod tests {
    use super::{DeviceAttributeTracker, DA1_RESPONSE, DA2_RESPONSE};

    #[test]
    fn tracker_detects_queries_split_across_chunks() {
        let mut tracker = DeviceAttributeTracker::new();
        assert!(tracker.push(b"\x1b[").is_empty());
        assert_eq!(tracker.push(b"c"), vec![DA1_RESPONSE.to_vec()]);
        assert!(tracker.push(b"\x1b[>").is_empty());
        assert_eq!(tracker.push(b"0c"), vec![DA2_RESPONSE.to_vec()]);
    }
}
