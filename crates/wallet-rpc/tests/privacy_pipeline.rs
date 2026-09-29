//! Full SDK child -> Rust signer -> proof callback -> review -> one-shot submit.
//! Public test seed and synthetic loopback chain only; never a live transaction.
use axum::{extract::State, routing::post, Json, Router};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::Mutex;
use wallet_core::{
    AccountRef, Call, ChainId, Domain, Felt, InvokeV3Params, Registry, ResourceBounds,
};
use wallet_rpc::{privacy, AutoApprover, Decision, HttpStarknetRpc, ServerState, WalletSession};
const SEED: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
#[derive(Clone, Default)]
struct Chain {
    broadcasts: Arc<AtomicUsize>,
    accepted: Arc<AtomicBool>,
}
async fn rpc(State(st): State<Chain>, Json(v): Json<Value>) -> Json<Value> {
    let p = &v["params"];
    let result = match v["method"].as_str().unwrap() {
        "starknet_chainId" => json!("0x534e5f4d41494e"),
        "starknet_blockNumber" => json!(1000),
        "starknet_getClassHashAt" => json!("0x99"),
        "starknet_getNonce" => json!("0xa"),
        "starknet_call" => {
            let selector =
                Felt::from_hex(p["request"]["entry_point_selector"].as_str().unwrap()).unwrap();
            if selector == wallet_core::get_selector_from_name("get_public_key") {
                json!(["0x0"])
            } else if selector == wallet_core::get_selector_from_name("get_fee_amount") {
                json!(["0x53444835ec580000"])
            } else if selector == wallet_core::get_selector_from_name("get_proof_validity_blocks") {
                json!(["0x1c2"])
            } else {
                assert_eq!(selector, wallet_core::get_selector_from_name("balanceOf"));
                json!(["0x56bc75e2d63100000", "0x0"])
            }
        }
        "starknet_estimateFee" => {
            assert_eq!(p["request"][0]["proof"], "synthetic-proof");
            json!([{"unit":"FRI","l1_gas_consumed":"0x1","l1_gas_price":"0x2","l2_gas_consumed":"0xa","l2_gas_price":"0x3","l1_data_gas_consumed":"0x1","l1_data_gas_price":"0x4"}])
        }
        "starknet_addInvokeTransaction" => {
            st.broadcasts.fetch_add(1, Ordering::SeqCst);
            return Json(
                json!({"jsonrpc":"2.0","id":v["id"],"error":{"code":-32000,"message":"Synthetic uncertain submission"}}),
            );
        }
        "starknet_getTransactionReceipt" => {
            if !st.accepted.load(Ordering::SeqCst) {
                return Json(
                    json!({"jsonrpc":"2.0","id":v["id"],"error":{"code":29,"message":"not found"}}),
                );
            }
            json!({"execution_status":"SUCCEEDED","finality_status":"ACCEPTED_ON_L2","block_number":991,"actual_fee":{"amount":"0x5","unit":"FRI"}})
        }
        m => panic!("unexpected method {m}"),
    };
    Json(json!({"jsonrpc":"2.0","id":v["id"],"result":result}))
}
struct Proof {
    pubkey: Felt,
}
#[async_trait::async_trait]
impl prover::Prover for Proof {
    async fn prove(&self, req: prover::ProveRequest) -> Result<prover::ProveResult, String> {
        assert_eq!(req.payload["block_number"], 990);
        let tx = &req.payload["transaction"];
        let f = |v: &Value| Felt::from_hex(v.as_str().unwrap()).unwrap();
        let calldata = tx["calldata"].as_array().unwrap();
        assert_eq!(f(&calldata[0]), Felt::ONE);
        let calls = vec![Call {
            to: f(&calldata[1]),
            selector: f(&calldata[2]),
            calldata: calldata[4..].iter().map(f).collect(),
        }];
        let pool = f(&tx["sender_address"]);
        assert_eq!(calls[0].to, pool);
        let params = InvokeV3Params {
            nonce: Felt::from(10u64),
            tip: 0,
            l1_gas: ResourceBounds {
                max_amount: 1,
                max_price_per_unit: 0,
            },
            l2_gas: ResourceBounds {
                max_amount: 100_000_000,
                max_price_per_unit: 0,
            },
            l1_data_gas: ResourceBounds {
                max_amount: 1,
                max_price_per_unit: 0,
            },
            ..Default::default()
        };
        let hash = wallet_core::invoke_v3_hash(&pool, &calls, ChainId::Mainnet, &params);
        assert!(starknet_crypto::verify(
            &self.pubkey,
            &hash,
            &f(&tx["signature"][0]),
            &f(&tx["signature"][1])
        )
        .unwrap());
        Ok(prover::ProveResult {
            proof: json!({"proof":"synthetic-proof","proof_facts":["0x1"],"l2_to_l1_messages":[{"from_address":format!("{pool:#x}"),"payload":["0x9","0x0"]}]}),
        })
    }
    fn kind(&self) -> &'static str {
        "starknet-rpc"
    }
    fn ready(&self) -> bool {
        true
    }
}
#[tokio::test]
#[ignore = "requires Node.js 24+ and npm --prefix privacy run build"]
async fn real_sdk_child_signing_approval_replay_and_uncertain_receipt_recovery() {
    pipeline(false).await;
}
#[tokio::test]
#[ignore = "requires Node.js 24+ and npm --prefix privacy run build"]
async fn desktop_one_click_enforces_fee_caps_without_relaxing_review_submission() {
    pipeline(true).await;
}
async fn pipeline(desktop: bool) {
    let fixture = Chain::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/", post(rpc))
        .with_state(fixture.clone());
    let http = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let dir = std::env::temp_dir().join(format!(
        "gravity-privacy-pipeline-{}",
        (std::process::id() as u64 * 2 + u64::from(desktop))
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let settings = Arc::new(prover::SettingsStore::load(
        dir.join("prover-settings.json"),
    ));
    settings
        .update(prover::Settings {
            mainnet: prover::NetworkConfig {
                rpc_url: url.clone(),
                prover_url: url.clone(),
                prover_api_key: String::new(),
            },
            prover_backend: "starknet-rpc".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let pubkey = wallet_core::public_key(SEED, Domain::User, 0, None).unwrap();
    let prover = Arc::new(prover::ProverState {
        prover: Arc::new(Proof { pubkey }),
        jobs: Arc::new(prover::Jobs::new(0)),
        settings,
        storage: Arc::new(prover::Storage::new(dir.join("proofs"))),
    });
    let address = wallet_core::address_hex(
        &wallet_core::oz_address(SEED, Domain::User, 0, None, ChainId::Mainnet).unwrap(),
    );
    let mut reg = Registry::default();
    reg.add(AccountRef {
        domain: Domain::User,
        index: 0,
        address: address.clone(),
        label: "Test".into(),
        owner_client_id: None,
    });
    let session = Arc::new(Mutex::new(WalletSession::new_unlocked(
        ChainId::Mainnet,
        SEED,
        "test",
        reg,
    )));
    let state = ServerState::new(
        session.clone(),
        Arc::new(AutoApprover(if desktop {
            Decision::Reject
        } else {
            Decision::Approve
        })),
    )
    .with_node(ChainId::Mainnet, Arc::new(HttpStarknetRpc::new(url)))
    .with_prover(prover);
    let worker = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../desktop/src-tauri/resources/privacy/worker.cjs");
    let state = privacy::attach(state, dir.join("privacy"), worker);
    let req = privacy::Request {
        account: address.clone(),
        chain_id: "0x534e5f4d41494e".into(),
        mode: "prepare".into(),
        operation: "register".into(),
        amount: "0".into(),
        recipient: address.clone(),
    };
    // An agent cannot prepare for an unrelated user account.
    assert!(
        privacy::run(&state, "foreign", Some("foreign"), req.clone(), false)
            .await
            .is_err()
    );
    let review = privacy::run(&state, "desktop", None, req.clone(), false)
        .await
        .unwrap();
    assert_eq!(review["max_network_fee"], "93");
    assert!(review.get("proof").is_none());
    assert!(review.get("viewing_key").is_none());
    let id = review["review_id"].as_str().unwrap();
    assert!(privacy::submit(&state, "foreign", None, id).await.is_err()); // does not consume owner's review
    let sent = if desktop {
        // The normal review endpoint still requires approval, even for desktop.
        assert!(matches!(
            privacy::submit(&state, "desktop", None, id).await,
            Err(wallet_rpc::WalletRpcError::UserRefused)
        ));
        for (pool, network) in [
            ("6000000000000000000", "92"),
            ("5000000000000000000", "93"),
            ("6000000000000000000", "0"),
        ] {
            assert!(privacy::execute_desktop(
                &state,
                req.clone(),
                privacy::DesktopLimits {
                    max_pool_fee: pool.into(),
                    max_network_fee: network.into(),
                }
            )
            .await
            .is_err());
            assert_eq!(fixture.broadcasts.load(Ordering::SeqCst), 0);
        }
        // Exact fee ceilings pass without consulting the rejecting approver.
        privacy::execute_desktop(
            &state,
            req.clone(),
            privacy::DesktopLimits {
                max_pool_fee: "6000000000000000000".into(),
                max_network_fee: "93".into(),
            },
        )
        .await
        .unwrap()
    } else {
        privacy::submit(&state, "desktop", None, id).await.unwrap()
    };
    assert_eq!(sent["status"], "submission_unknown");
    assert_eq!(fixture.broadcasts.load(Ordering::SeqCst), 1);
    assert!(privacy::submit(&state, "desktop", None, id).await.is_err());
    assert!(privacy::run(&state, "desktop", None, req.clone(), false)
        .await
        .is_err());
    let tx = sent["transaction_hash"].as_str().unwrap();
    assert!(privacy::receipt(&state, tx, ChainId::Mainnet)
        .await
        .is_err());
    fixture.accepted.store(true, Ordering::SeqCst);
    let receipt = privacy::receipt(&state, tx, ChainId::Mainnet)
        .await
        .unwrap();
    assert_eq!(receipt["execution_status"], "SUCCEEDED");
    let history = privacy::history(&state, None, &address, ChainId::Mainnet)
        .await
        .unwrap();
    assert_eq!(history[0]["status"], "SUCCEEDED");
    // A receipt is final but still younger than the mature proof base.
    assert!(privacy::run(&state, "desktop", None, req, false)
        .await
        .is_err());
    assert_eq!(fixture.broadcasts.load(Ordering::SeqCst), 1);
    // Only final public proof output is persisted by the remote backend.
    let record = std::fs::read_to_string(dir.join("proofs/p0.json")).unwrap();
    let record: Value = serde_json::from_str(&record).unwrap();
    assert!(record["payload"].is_null());
    http.abort();
    std::fs::remove_dir_all(dir).unwrap();
}
