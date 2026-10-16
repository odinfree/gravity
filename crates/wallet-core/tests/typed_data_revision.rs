//! Keep Gravity's advertised SNIP-12 revision-1 boundary across KMS upgrades.
use serde_json::{json, Value};
use wallet_core::{typed_data_message_hash, Felt};

#[test]
fn only_explicit_revision_one_is_accepted() {
    let account = Felt::from(0x123u64);
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/snip12-canonical.json")).unwrap();
    let base = fixture["vectors"][0]["typed_data"].clone();
    for revision in [json!("1"), json!(1)] {
        let mut doc = base.clone();
        doc["domain"]["revision"] = revision;
        let json = doc.to_string();
        let expected = krusty_kms::compute_typed_data_message_hash(&json, &account).unwrap();
        assert_eq!(typed_data_message_hash(&json, &account).unwrap(), expected);
    }

    for revision in [Some(json!("0")), Some(json!(0)), None] {
        let mut doc = json!({
            "types":{
                "StarkNetDomain":[
                    {"name":"name","type":"felt"},
                    {"name":"version","type":"felt"},
                    {"name":"chainId","type":"felt"}
                ],
                "Message":[{"name":"value","type":"felt"}]
            },
            "primaryType":"Message",
            "domain":{"name":"Review","version":"1","chainId":"SN_MAIN"},
            "message":{"value":"42"}
        });
        if let Some(revision) = revision {
            doc["domain"]["revision"] = revision;
        }
        let json = doc.to_string();
        assert!(krusty_kms::compute_typed_data_message_hash(&json, &account).is_ok());
        assert!(typed_data_message_hash(&json, &account).is_err());
    }
}
