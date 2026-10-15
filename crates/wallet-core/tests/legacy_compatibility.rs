//! Regression snapshots captured BEFORE the upstream dependency update from
//! Gravity 4beb20a. Public test seed only. Do not regenerate to bless an upgrade:
//! these pin Gravity's identity, not upstream's different address conventions.
use serde_json::Value;
use wallet_core::*;

#[test]
fn existing_accounts_and_privacy_keys_keep_their_identity() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/legacy-wallet-v1.json")).unwrap();
    let seed = fixture["mnemonic"].as_str().unwrap();
    let pool = Felt::from_hex(fixture["pool"].as_str().unwrap()).unwrap();
    let calls = vec![Call {
        to: Felt::from(456u64),
        selector: get_selector_from_name("transfer"),
        calldata: vec![Felt::from(789u64), Felt::from(42u64), Felt::ZERO],
    }];
    let params = InvokeV3Params {
        nonce: Felt::from(7u64),
        l2_gas: ResourceBounds {
            max_amount: 100_000,
            max_price_per_unit: 10,
        },
        proof_facts: vec![Felt::ONE],
        ..Default::default()
    };
    let vectors = fixture["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 24);
    for v in vectors {
        let chain = match v["chain"].as_str().unwrap() {
            "mainnet" => ChainId::Mainnet,
            "sepolia" => ChainId::Sepolia,
            _ => panic!("invalid fixture network"),
        };
        let domain: Domain = serde_json::from_value(v["domain"].clone()).unwrap();
        let index = u32::try_from(v["index"].as_u64().unwrap()).unwrap();
        let pass = v["passphrase"].as_str();
        let d = deployment_data(seed, domain, index, pass, chain).unwrap();
        assert_eq!(domain.path(index), v["path"]);
        assert_eq!(
            address_hex(&public_key(seed, domain, index, pass).unwrap()),
            v["public_key"]
        );
        assert_eq!(address_hex(&d.address), v["address"]);
        assert_eq!(address_hex(&d.class_hash), v["class_hash"]);
        assert_eq!(address_hex(&d.salt), v["salt"]);
        assert_eq!(
            serde_json::json!(d
                .constructor_calldata
                .iter()
                .map(address_hex)
                .collect::<Vec<_>>()),
            v["constructor_calldata"]
        );
        assert_eq!(
            privacy::viewing_key_v1(seed, domain, index, chain, &pool)
                .unwrap()
                .as_str(),
            v["viewing_key"]
        );
        let signed = sign_invoke_v3(
            seed, domain, index, pass, &d.address, &calls, chain, &params,
        )
        .unwrap();
        assert_eq!(address_hex(&signed.transaction_hash), v["invoke_hash"]);
        assert_eq!(
            serde_json::json!([address_hex(&signed.r), address_hex(&signed.s)]),
            v["signature"]
        );
    }
}
