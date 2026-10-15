//! SNIP-12 vectors generated independently with the pinned starknet.js runtime.
use serde_json::Value;
use wallet_core::{typed_data_message_hash, Felt};

#[test]
fn string_and_selector_digests_match_starknet_js() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/snip12-canonical.json")).unwrap();
    let vectors = fixture["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 12);
    for v in vectors {
        let address = Felt::from_hex(v["account_address"].as_str().unwrap()).unwrap();
        let expected = Felt::from_hex(v["message_hash"].as_str().unwrap()).unwrap();
        let got = typed_data_message_hash(&v["typed_data"].to_string(), &address);
        if v["accepted"] == false {
            assert!(
                got.is_err(),
                "pre-hashed selectors must not be signed with a different digest"
            );
        } else {
            assert_eq!(got.unwrap(), expected, "public reference vector: {v}");
        }
    }
}
