use cleat::da::{device_attribute_replies, DA1_RESPONSE, DA2_RESPONSE};

#[test]
fn detects_da1_and_da2_queries_without_matching_responses() {
    let replies = device_attribute_replies(b"\x1b[c\x1b[>0c\x1b[?62;22c");

    assert_eq!(replies, vec![DA1_RESPONSE.to_vec(), DA2_RESPONSE.to_vec()]);
}

#[test]
fn ignores_partial_or_non_query_sequences() {
    assert!(device_attribute_replies(b"\x1b[").is_empty());
    assert!(device_attribute_replies(b"\x1b[?62;22c").is_empty());
}

#[test]
fn detects_explicit_and_implicit_query_variants() {
    assert_eq!(device_attribute_replies(b"\x1b[0c"), vec![DA1_RESPONSE.to_vec()]);
    assert_eq!(device_attribute_replies(b"\x1b[>c"), vec![DA2_RESPONSE.to_vec()]);
}
