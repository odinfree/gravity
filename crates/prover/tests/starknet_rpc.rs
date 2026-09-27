//! Synthetic protocol tests. No wallet keys, real proofs, or chain writes.
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use prover::{
    build_prover_state, enqueue_prove, JobStatus, NetworkConfig, ProveRequest, Prover,
    ProverConfig, Settings, SettingsStore, StarknetRpcProver,
};
use serde_json::{json, Value};

static SEQ: AtomicUsize = AtomicUsize::new(0);
type Seen = Arc<Mutex<Vec<(String, Value, bool)>>>;

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "strkd-rpc-test-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Default)]
struct Behavior {
    wrong_chain: bool,
    proof_reply: Option<Value>,
    redirect: bool,
}
struct Server {
    url: String,
    seen: Seen,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn reply(
    State((behavior, seen)): State<(Behavior, Seen)>,
    Path(path): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> axum::response::Response {
    seen.lock().unwrap().push((
        path.clone(),
        body.clone(),
        headers.contains_key("x-api-key"),
    ));
    if behavior.redirect && path == "prover" {
        return (StatusCode::TEMPORARY_REDIRECT, [("location", "/target")]).into_response();
    }
    let result = match body["method"].as_str().unwrap() {
        "starknet_chainId" => json!(if behavior.wrong_chain {
            "0x534e5f4d41494e"
        } else {
            "0x534e5f5345504f4c4941"
        }),
        "starknet_specVersion" => json!("0.10.3-rc.2"),
        "starknet_blockNumber" => json!(500),
        "starknet_proveTransaction" => {
            if let Some(v) = behavior.proof_reply {
                return Json(v).into_response();
            }
            proof()
        }
        _ => panic!("adapter sent an unexpected method"),
    };
    Json(json!({"jsonrpc":"2.0", "id":1, "result":result})).into_response()
}

async fn server(behavior: Behavior) -> Server {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/:path", post(reply))
        .with_state((behavior, seen.clone()));
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Server { url, seen, task }
}

async fn settings(dir: &TempDir, url: &str) -> Arc<SettingsStore> {
    let store = Arc::new(SettingsStore::load(dir.0.join("settings.json")));
    store
        .update(Settings {
            testnet: NetworkConfig {
                rpc_url: format!("{url}/node"),
                prover_url: format!("{url}/prover"),
                // A stale legacy API key must not leak into this backend's requests.
                prover_api_key: "SYNTHETIC_LEGACY_KEY".into(),
            },
            prover_backend: "starknet-rpc".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    store
}

fn transaction() -> Value {
    let bound = json!({"max_amount":"0x100", "max_price_per_unit":"0x0"});
    json!({"type":"INVOKE", "version":"0x3", "sender_address":"0x123", "nonce":"0x0",
        "signature":["0x1","0x2"], "calldata":["0xabc"], "tip":"0x0",
        "resource_bounds":{"l1_gas":bound, "l2_gas":bound, "l1_data_gas":bound},
        "paymaster_data":[], "account_deployment_data":[],
        "nonce_data_availability_mode":"L1", "fee_data_availability_mode":"L1"})
}
fn proof() -> Value {
    json!({"proof":"c3ludGhldGljLXRlc3Qtb25seQ==", "proof_facts":["0x1"],
        "l2_to_l1_messages":[{"from_address":"0x123","to_address":"0x456","payload":["0x789"]}],
        "additional_data":{"test_only":true}})
}
fn request() -> ProveRequest {
    ProveRequest {
        payload: json!({"transaction":transaction(),"block_number":400}),
        label: None,
        network: "testnet".into(),
    }
}

#[tokio::test]
async fn routes_unchanged_signed_transaction_only_to_configured_prover() {
    let server = server(Behavior::default()).await;
    let dir = TempDir::new();
    let adapter = StarknetRpcProver::new(settings(&dir, &server.url).await);
    assert!(adapter.ready());
    assert_eq!(adapter.prove(request()).await.unwrap().proof, proof());
    let seen = server.seen.lock().unwrap();
    let proves: Vec<_> = seen
        .iter()
        .filter(|(_, b, _)| b["method"] == "starknet_proveTransaction")
        .collect();
    assert_eq!(proves.len(), 1);
    assert_eq!(proves[0].0, "prover"); // No legacy /v1/prove suffix.
    assert_eq!(
        proves[0].1["params"],
        json!({"block_id":{"block_number":400},"transaction":transaction()})
    );
    assert!(seen.iter().all(|(_, _, api_key)| !api_key));
    assert!(seen
        .iter()
        .filter(|(p, _, _)| p == "node")
        .all(|(_, b, _)| b["params"] == json!([])));
}

#[tokio::test]
async fn omitted_block_uses_head_minus_ten() {
    let server = server(Behavior::default()).await;
    let dir = TempDir::new();
    let adapter = StarknetRpcProver::new(settings(&dir, &server.url).await);
    let mut req = request();
    req.payload.as_object_mut().unwrap().remove("block_number");
    adapter.prove(req).await.unwrap();
    let seen = server.seen.lock().unwrap();
    let (_, body, _) = seen
        .iter()
        .find(|(_, b, _)| b["method"] == "starknet_proveTransaction")
        .unwrap();
    assert_eq!(body["params"]["block_id"], json!({"block_number":490}));
}

#[tokio::test]
async fn wrong_node_network_prevents_private_payload_delivery() {
    let server = server(Behavior {
        wrong_chain: true,
        ..Default::default()
    })
    .await;
    let dir = TempDir::new();
    let adapter = StarknetRpcProver::new(settings(&dir, &server.url).await);
    assert!(adapter
        .prove(request())
        .await
        .unwrap_err()
        .contains("network"));
    assert_eq!(server.seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn invalid_signed_payloads_and_unknown_network_never_reach_the_wire() {
    let server = server(Behavior::default()).await;
    let dir = TempDir::new();
    let adapter = StarknetRpcProver::new(settings(&dir, &server.url).await);
    let mut unsigned = request();
    unsigned.payload["transaction"]["signature"] = json!([]);
    let mut fee = request();
    fee.payload["transaction"]["resource_bounds"]["l2_gas"]["max_price_per_unit"] = json!("0x1");
    let mut tip = request();
    tip.payload["transaction"]["tip"] = json!("0x1");
    let mut block = request();
    block.payload["block_number"] = json!("SYNTHETIC_PRIVATE_INPUT");
    let mut network = request();
    network.network = "unknown".into();
    for req in [unsigned, fee, tip, block, network] {
        assert!(adapter.prove(req).await.is_err());
    }
    assert!(server.seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn upstream_errors_and_malformed_results_are_not_echoed_or_retried() {
    for body in [
        json!({"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"SYNTHETIC_PRIVATE_INPUT"}}),
        json!({"jsonrpc":"2.0","id":1,"error":{"code":-32005,"data":"SYNTHETIC_PRIVATE_INPUT"}}),
        json!({"jsonrpc":"2.0","id":99,"result":proof()}),
        json!({"jsonrpc":"2.0","id":1,"result":{"proof":"SYNTHETIC_PRIVATE_INPUT"}}),
    ] {
        let server = server(Behavior {
            proof_reply: Some(body),
            ..Default::default()
        })
        .await;
        let dir = TempDir::new();
        let adapter = StarknetRpcProver::new(settings(&dir, &server.url).await);
        let error = adapter.prove(request()).await.unwrap_err();
        assert!(!error.contains("SYNTHETIC_PRIVATE_INPUT"));
        assert_eq!(
            server
                .seen
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, b, _)| b["method"] == "starknet_proveTransaction")
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn redirects_are_not_followed() {
    let server = server(Behavior {
        redirect: true,
        ..Default::default()
    })
    .await;
    let dir = TempDir::new();
    let adapter = StarknetRpcProver::new(settings(&dir, &server.url).await);
    assert!(adapter
        .check_connection("testnet")
        .await
        .unwrap_err()
        .contains("307"));
    assert_eq!(server.seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn invalid_endpoints_fail_without_disclosing_credentials() {
    let dir = TempDir::new();
    let store = Arc::new(SettingsStore::load(dir.0.join("settings.json")));
    let adapter = StarknetRpcProver::new(store.clone());
    for url in [
        "http://example.invalid/SYNTHETIC_CREDENTIAL",
        "https://user:SYNTHETIC_CREDENTIAL@example.invalid",
        "https://example.invalid/SYNTHETIC_CREDENTIAL bad",
    ] {
        store
            .update(Settings {
                testnet: NetworkConfig {
                    prover_url: url.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .await
            .unwrap();
        let error = adapter.check_connection("testnet").await.unwrap_err();
        assert!(!error.contains("SYNTHETIC_CREDENTIAL"));
    }
}

#[tokio::test]
async fn selected_backend_uses_json_rpc_and_does_not_persist_private_calldata() {
    let server = server(Behavior::default()).await;
    let dir = TempDir::new();
    settings(&dir, &server.url).await;
    // The persisted selection wins over the native default.
    let state = build_prover_state(
        dir.0.clone(),
        &ProverConfig {
            prover_backend: "native".into(),
        },
    );
    assert_eq!(state.prover.kind(), "starknet-rpc");
    let id = enqueue_prove(&state, request().payload, None, "testnet".into()).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let job = state.jobs.get(&id).await.unwrap();
            if job.status == JobStatus::Failed {
                panic!("synthetic prove failed: {:?}", job.error);
            }
            if let Some(record) = state.storage.get_record(&id) {
                assert_eq!(record.status, "succeeded");
                assert!(record.payload.is_null());
                assert_eq!(record.proof, Some(proof()));
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "read-only check requiring the operator's existing loopback prover"]
async fn live_loopback_prover_health() {
    let dir = TempDir::new();
    let store = Arc::new(SettingsStore::load(dir.0.join("settings.json")));
    store
        .update(Settings {
            mainnet: NetworkConfig {
                prover_url: "http://127.0.0.1:3000".into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .unwrap();
    let version = StarknetRpcProver::new(store)
        .check_connection("mainnet")
        .await
        .unwrap();
    println!("Existing loopback prover RPC version: {version}; no proof requested");
}
