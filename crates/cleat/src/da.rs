pub const DA1_RESPONSE: &[u8] = b"\x1b[?62;22c";
pub const DA2_RESPONSE: &[u8] = b"\x1b[>1;10;0c";

const DA1_QUERY: &[u8] = b"\x1b[c";
const DA1_QUERY_EXPLICIT: &[u8] = b"\x1b[0c";
const DA2_QUERY: &[u8] = b"\x1b[>c";
const DA2_QUERY_EXPLICIT: &[u8] = b"\x1b[>0c";

pub fn device_attribute_replies(data: &[u8]) -> Vec<Vec<u8>> {
    let mut replies = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\x1b' && i + 1 < data.len() && data[i + 1] == b'[' {
            if i + 2 < data.len() && data[i + 2] == b'?' {
                i += 3;
                continue;
            }
            if matches_at(&data[i..], DA2_QUERY) || matches_at(&data[i..], DA2_QUERY_EXPLICIT) {
                replies.push(DA2_RESPONSE.to_vec());
            } else if matches_at(&data[i..], DA1_QUERY) || matches_at(&data[i..], DA1_QUERY_EXPLICIT) {
                replies.push(DA1_RESPONSE.to_vec());
            }
        }
        i += 1;
    }
    replies
}

fn matches_at(data: &[u8], needle: &[u8]) -> bool {
    data.len() >= needle.len() && &data[..needle.len()] == needle
}
