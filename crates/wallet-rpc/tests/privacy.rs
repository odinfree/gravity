use serde_json::{json, Value};
use wallet_rpc::{
    dispatch::STRK_TOKEN_ADDRESS,
    privacy::{validate_endpoint, validate_prepared, Request},
    LogEntry, RequestLog,
};
const POOL: &str = "0x123";
fn fixture() -> (Request, Value, Value) {
    let req = Request {
        account: "0x456".into(),
        chain_id: "0x534e5f4d41494e".into(),
        mode: "prepare".into(),
        operation: "deposit".into(),
        amount: "10".into(),
        recipient: "0x456".into(),
    };
    let config = json!({"pool_address":POOL});
    let zero = json!({"max_amount":"0x0","max_price_per_unit":"0x0"});
    let result = json!({"operation":"deposit","account":"0x456","recipient":"0x456","chain_id":req.chain_id,
        "pool_address":POOL,"token":STRK_TOKEN_ADDRESS,"amount":"10","pool_fee":"6","screening_attached":true,
        "max_network_fee":"2","proof":"synthetic-proof","proof_facts":["0x1"],
        "resource_bounds":{"l1_gas":zero,"l2_gas":{"max_amount":"0x1","max_price_per_unit":"0x2"},"l1_data_gas":zero},
        "calls":[{"contract_address":STRK_TOKEN_ADDRESS,"entrypoint":"approve","calldata":[POOL,"0x10","0x0"]},
                 {"contract_address":POOL,"entrypoint":"apply_actions","calldata":["0x1"]}]});
    (req, config, result)
}
#[test]
fn exact_review_accepts_only_matching_operation_and_allowance() {
    let (req, cfg, r) = fixture();
    assert!(validate_prepared(&req, &cfg, &r).is_ok());
    for (key, value) in [
        ("account", json!("0x999")),
        ("recipient", json!("0x999")),
        ("chain_id", json!("0x534e5f5345504f4c4941")),
        ("pool_address", json!("0x999")),
        ("token", json!("0x999")),
        ("amount", json!("11")),
        ("screening_attached", json!(false)),
        ("max_network_fee", json!("3")),
        ("proof", json!("")),
        ("proof_facts", json!([])),
    ] {
        let mut changed = r.clone();
        changed[key] = value;
        assert!(
            validate_prepared(&req, &cfg, &changed).is_err(),
            "accepted changed {key}"
        );
    }
    let mut changed = r.clone();
    changed["calls"][0]["calldata"][1] = json!("0xffffffffffffffffffffffffffffffff");
    assert!(validate_prepared(&req, &cfg, &changed).is_err());
    let mut changed = r.clone();
    changed["calls"][1]["entrypoint"] = json!("transfer");
    assert!(validate_prepared(&req, &cfg, &changed).is_err());
    let mut changed = r;
    changed["calls"]
        .as_array_mut()
        .unwrap()
        .push(json!({"contract_address":"0x9","entrypoint":"transfer","calldata":[]}));
    assert!(validate_prepared(&req, &cfg, &changed).is_err());
}
#[test]
fn private_endpoint_validation() {
    for s in [
        "https://example.test/rpc",
        "http://127.0.0.1:3000",
        "http://[::1]:8080",
    ] {
        assert!(validate_endpoint(s).is_ok());
    }
    for s in [
        "http://example.test",
        "https://user:secret@example.test",
        "https://example.test/#x",
        "file:///tmp/key",
    ] {
        assert!(validate_endpoint(s).is_err());
    }
}
#[test]
fn privacy_requests_never_persist_private_fields_in_debug_logs() {
    let log = RequestLog::in_memory(true).unwrap();
    for method in [
        "companion_privacyPrepare",
        "companion_privacyBalances",
        "companion_privacySubmit",
        "wallet_strk20PrepareInvoke",
    ] {
        log.record(LogEntry {
            ts_unix_ms: 1,
            method: method.into(),
            client: None,
            network: None,
            decision: "approved".into(),
            outcome: "synthetic-private-error".into(),
            error_code: Some(123),
            latency_ms: 1,
            params_json: Some("synthetic-private-input".into()),
            result_json: Some("synthetic-private-output".into()),
        });
    }
    for row in log.recent(10) {
        assert!(row.params_json.is_none());
        assert!(row.result_json.is_none());
        assert_eq!(row.outcome, "error 123");
    }
}
