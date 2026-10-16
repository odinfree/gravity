//! Enforce the selector-name policy across every reference-encoder enum shape.
//! These documents use public values; no signing key or installed vault is read.
use serde_json::{json, Value};
use wallet_core::{typed_data_message_hash, Felt};

const CONTEXTS: [&str; 5] = ["direct", "tuple", "array", "merkletree", "primary"];

fn document(context: &str, multiple: bool, empty: bool, selector: &str) -> Value {
    let mut types = json!({
        "StarknetDomain": [
            {"name":"name","type":"shortstring"},
            {"name":"version","type":"shortstring"},
            {"name":"chainId","type":"shortstring"},
            {"name":"revision","type":"shortstring"}
        ],
        "Inner": if empty {
            json!([{"name":"None","type":"()"}])
        } else {
            json!([{"name":"Call","type":"(selector)"}])
        }
    });
    if multiple {
        types["Inner"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"Other","type":"(selector*)"}));
    }
    let inner = if empty {
        json!({"None":[]})
    } else {
        json!({"Call":[selector]})
    };
    let (primary_type, message) = match context {
        "direct" => {
            types["Message"] = json!([{"name":"call","type":"enum","contains":"Inner"}]);
            ("Message", json!({"call":inner}))
        }
        "tuple" => {
            types["Outer"] = json!([{"name":"Wrap","type":"(Inner)"}]);
            types["Message"] = json!([{"name":"call","type":"enum","contains":"Outer"}]);
            ("Message", json!({"call":{"Wrap":[inner]}}))
        }
        "array" => {
            types["Message"] = json!([{"name":"calls","type":"Inner*"}]);
            ("Message", json!({"calls":[inner]}))
        }
        "merkletree" => {
            types["Message"] = json!([{"name":"calls","type":"merkletree","contains":"Inner"}]);
            ("Message", json!({"calls":[inner]}))
        }
        "primary" => ("Inner", inner),
        _ => unreachable!(),
    };
    json!({
        "types":types,"primaryType":primary_type,
        "domain":{"name":"Review","version":"1","chainId":"SN_MAIN","revision":"1"},
        "message":message
    })
}

#[test]
fn named_enum_references_apply_selector_policy_to_the_selected_variant() {
    let account = Felt::from(0x123u64);
    for context in CONTEXTS {
        for multiple in [false, true] {
            for selector in ["transfer", "0x1234"] {
                let doc = document(context, multiple, false, selector).to_string();
                let reference = krusty_kms::compute_typed_data_message_hash(&doc, &account)
                    .expect("the reference encoder supports this enum document");
                let actual = typed_data_message_hash(&doc, &account);
                if selector == "transfer" {
                    assert_eq!(actual.unwrap(), reference, "{context}, multiple={multiple}");
                } else {
                    assert!(actual.is_err(), "{context}, multiple={multiple}");
                }
            }
        }
    }
}

#[test]
fn empty_variants_do_not_require_values_for_unselected_variants() {
    let account = Felt::from(0x123u64);
    for context in CONTEXTS {
        for multiple in [false, true] {
            let doc = document(context, multiple, true, "unused").to_string();
            let reference = krusty_kms::compute_typed_data_message_hash(&doc, &account)
                .expect("the reference encoder supports this empty variant");
            assert_eq!(
                typed_data_message_hash(&doc, &account).unwrap(),
                reference,
                "{context}, multiple={multiple}"
            );
        }
    }
}
