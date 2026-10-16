//! Synthetic treasury lifecycle only: public test mnemonic, temporary journals, mock RPC.
use async_trait::async_trait;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};
use tokio::sync::Mutex;
use wallet_core::{AccountRef, Call, ChainId, Domain, Felt, InvokeV3Params, Registry};
use wallet_rpc::{
    dispatch, AutoApprover, ChannelApprover, Decision, FeeBounds, NodeError, Request, ServerState,
    StarknetRpc, WalletSession,
};
const MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let mut b = [0u8; 8];
        getrandom::getrandom(&mut b).unwrap();
        let p = std::env::temp_dir().join(format!("gravity-treasury-test-{}", hex::encode(b)));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[derive(Default)]
struct Node {
    nonce: AtomicU64,
    broadcasts: AtomicU64,
    ambiguous: AtomicBool,
    wrong_hash: AtomicBool,
    wrong_chain: AtomicBool,
    accepted: AtomicBool,
    hold_broadcast: AtomicBool,
    broadcast_entered: tokio::sync::Notify,
    broadcast_release: tokio::sync::Notify,
}
#[async_trait]
impl StarknetRpc for Node {
    async fn chain_id(&self) -> Result<Felt, NodeError> {
        Ok(if self.wrong_chain.load(Ordering::SeqCst) {
            ChainId::Mainnet
        } else {
            ChainId::Sepolia
        }
        .as_felt())
    }
    async fn accepted_invoke(
        &self,
        hash: &Felt,
        sender: &Felt,
        nonce: &Felt,
    ) -> Result<bool, NodeError> {
        assert_ne!(*hash, Felt::ZERO);
        assert_eq!(
            *sender,
            wallet_core::oz_address(MNEMONIC, Domain::User, 0, None, ChainId::Sepolia).unwrap()
        );
        assert_eq!(*nonce, Felt::ZERO);
        Ok(self.accepted.load(Ordering::SeqCst))
    }
    async fn get_nonce(&self, _: &Felt) -> Result<Felt, NodeError> {
        Ok(Felt::from(self.nonce.load(Ordering::SeqCst)))
    }
    async fn is_deployed(&self, _: &Felt) -> Result<bool, NodeError> {
        Ok(true)
    }
    async fn balance_of(&self, _: &Felt, _: &Felt) -> Result<u128, NodeError> {
        Ok(u128::MAX)
    }
    async fn estimate_invoke(
        &self,
        _: &Felt,
        _: &[Felt],
        _: &Felt,
    ) -> Result<FeeBounds, NodeError> {
        panic!("No quote validation claimed")
    }
    async fn add_invoke(
        &self,
        sender: &Felt,
        calldata: &[Felt],
        signature: &[Felt],
        nonce: &Felt,
        bounds: &FeeBounds,
        facts: &[Felt],
        proof: Option<&str>,
    ) -> Result<Felt, NodeError> {
        self.broadcasts.fetch_add(1, Ordering::SeqCst);
        if self.hold_broadcast.load(Ordering::SeqCst) {
            self.broadcast_entered.notify_one();
            self.broadcast_release.notified().await;
        }
        assert_eq!(signature.len(), 2);
        assert!(facts.is_empty() && proof.is_none());
        if self.ambiguous.load(Ordering::SeqCst) {
            return Err(NodeError::Transport("credential-canary".into()));
        }
        if self.wrong_hash.load(Ordering::SeqCst) {
            return Ok(Felt::ONE);
        }
        let calls = calldata[1..]
            .as_chunks::<6>()
            .0
            .iter()
            .map(|c| {
                assert_eq!(c[2], Felt::from(3u64));
                Call {
                    to: c[0],
                    selector: c[1],
                    calldata: c[3..].to_vec(),
                }
            })
            .collect::<Vec<_>>();
        Ok(wallet_core::invoke_v3_hash(
            sender,
            &calls,
            ChainId::Sepolia,
            &InvokeV3Params {
                nonce: *nonce,
                tip: 0,
                l1_gas: bounds.l1_gas,
                l2_gas: bounds.l2_gas,
                l1_data_gas: bounds.l1_data_gas,
                ..Default::default()
            },
        ))
    }
    async fn estimate_deploy_account(
        &self,
        _: &Felt,
        _: &Felt,
        _: &[Felt],
        _: &Felt,
    ) -> Result<FeeBounds, NodeError> {
        unreachable!()
    }
    async fn add_deploy_account(
        &self,
        _: &Felt,
        _: &[Felt],
        _: &Felt,
        _: &[Felt],
        _: &FeeBounds,
    ) -> Result<Felt, NodeError> {
        unreachable!()
    }
    async fn estimate_declare(
        &self,
        _: &Felt,
        _: &Felt,
        _: &Value,
        _: &Felt,
    ) -> Result<FeeBounds, NodeError> {
        unreachable!()
    }
    async fn add_declare(
        &self,
        _: &Felt,
        _: &Felt,
        _: &Value,
        _: &[Felt],
        _: &Felt,
        _: &FeeBounds,
    ) -> Result<Felt, NodeError> {
        unreachable!()
    }
}
fn session() -> WalletSession {
    let addr = wallet_core::oz_address(MNEMONIC, Domain::User, 0, None, ChainId::Sepolia).unwrap();
    let mut reg = Registry::default();
    reg.add(AccountRef {
        domain: Domain::User,
        index: 0,
        address: wallet_core::address_hex(&addr),
        label: "Test treasury".into(),
        owner_client_id: None,
    });
    WalletSession::new_unlocked(ChainId::Sepolia, MNEMONIC, "test-only", reg)
}
fn make(t: &Temp, approver: Arc<dyn wallet_rpc::Approver>, node: Arc<Node>) -> Arc<ServerState> {
    Arc::new(
        ServerState::new(Arc::new(Mutex::new(session())), approver)
            .with_clients_path(t.0.join("clients.json"))
            .with_node(ChainId::Sepolia, node),
    )
}
async fn pair(s: &ServerState) -> (String, String) {
    s.clients
        .lock()
        .await
        .pair("Floodgate test", wallet_rpc::ClientKind::App)
}
async fn request(s: &ServerState) -> Value {
    let account = s.session.lock().await.registry().unwrap().accounts[0]
        .address
        .clone();
    json!({"operationId":"12345678-1234-1234-1234-123456789abc","plan_hash":"a".repeat(64),"chainId":"0x534e5f5345504f4c4941","account_address":format!("0x{:x}",Felt::from_hex(&account).unwrap()),"transfers":[{"recipient":"0x456","amount_low":"0x11","amount_high":"0x1"}],"nonce":"0x0","resource_bounds":{"l1_gas":{"max_amount":"0x1","max_price_per_unit":"0x2"},"l2_gas":{"max_amount":"0x1","max_price_per_unit":"0x3"},"l1_data_gas":{"max_amount":"0x1","max_price_per_unit":"0x4"}},"max_fee":"9","expires_at":wallet_rpc::now_unix_ms()+120000})
}
async fn rpc(s: &ServerState, token: &str, method: &str, p: Value) -> wallet_rpc::Response {
    dispatch(
        s,
        Some(token),
        Request {
            jsonrpc: Some("2.0".into()),
            id: json!(1),
            method: method.into(),
            params: p,
        },
    )
    .await
}
async fn transfer(s: &ServerState, token: &str, p: Value) -> Value {
    let r = rpc(s, token, "companion_treasuryTransfer", p).await;
    assert!(r.error.is_none(), "{:?}", r.error);
    r.result.unwrap()
}
fn launch(s: Arc<ServerState>, token: String, p: Value) -> tokio::task::JoinHandle<Value> {
    tokio::spawn(async move { transfer(&s, &token, p).await })
}

#[test]
fn cross_language_fixture_and_hash_binding() {
    let f: Value = serde_json::from_str(include_str!("fixtures/treasury-intent-v1.json")).unwrap();
    let req: wallet_rpc::treasury::TransferRequest =
        serde_json::from_value(f["request"].clone()).unwrap();
    assert_eq!(wallet_rpc::treasury::intent_hash(&req), f["intentHash"]);
    for key in [
        "nonce",
        "max_fee",
        "plan_hash",
        "account_address",
        "chainId",
        "operationId",
    ] {
        let mut v = f["request"].clone();
        v[key] = json!("changed");
        let r = serde_json::from_value(v).unwrap();
        assert_ne!(wallet_rpc::treasury::intent_hash(&r), f["intentHash"]);
    }
}
#[tokio::test]
async fn full_review_ignores_grants_and_submits_exactly_once() {
    let t = Temp::new();
    let (a, mut rx) = ChannelApprover::new(4);
    let node = Arc::new(Node::default());
    let s = make(&t, Arc::new(a), node.clone());
    let (id, token) = pair(&s).await;
    s.clients
        .lock()
        .await
        .grant(&id, wallet_rpc::now_unix_ms() + 600000);
    let p = request(&s).await;
    let task = launch(s.clone(), token.clone(), p.clone());
    let approval = rx.recv().await.unwrap();
    for text in [
        "SN_SEPOLIA",
        "340282366920938463463374607431768211473",
        "0x456",
        "Maximum fee: 9",
        "Nonce: 0x0",
        "Intent SHA256",
        "L1 data gas",
        "Expires at",
    ] {
        assert!(approval.request.summary.contains(text), "missing {text}");
    }
    assert_eq!(node.broadcasts.load(Ordering::SeqCst), 0);
    approval.respond.send(Decision::Approve).unwrap();
    let result = task.await.unwrap();
    assert_eq!(result["status"], "submitted");
    assert!(result["transactionHash"].is_string());
    assert!(result.get("signature").is_none());
    assert_eq!(transfer(&s, &token, p.clone()).await, result);
    let mut changed = p;
    changed["max_fee"] = json!("10");
    assert!(rpc(&s, &token, "companion_treasuryTransfer", changed)
        .await
        .error
        .is_some());
    assert_eq!(node.broadcasts.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn rejection_and_expiry_are_durable_safe_terminal_states() {
    for expired in [false, true] {
        let t = Temp::new();
        let n = Arc::new(Node::default());
        let s = make(&t, Arc::new(AutoApprover(Decision::Reject)), n.clone());
        let (_, token) = pair(&s).await;
        let mut p = request(&s).await;
        if expired {
            p["expires_at"] = json!(wallet_rpc::now_unix_ms() - 1);
        }
        let r = transfer(&s, &token, p.clone()).await;
        assert_eq!(r["status"], if expired { "expired" } else { "rejected" });
        assert!(r["transactionHash"].is_null());
        drop(s);
        let reopened = make(&t, Arc::new(AutoApprover(Decision::Approve)), n.clone());
        assert_eq!(transfer(&reopened, &token, p).await, r);
        assert_eq!(n.broadcasts.load(Ordering::SeqCst), 0);
    }
}
#[tokio::test]
async fn lock_revoke_chain_nonce_and_session_replacement_cancel_after_review() {
    for change in ["lock", "revoke", "chain", "nonce", "replace", "nodechain"] {
        let t = Temp::new();
        let (a, mut rx) = ChannelApprover::new(1);
        let n = Arc::new(Node::default());
        let s = make(&t, Arc::new(a), n.clone());
        let (id, token) = pair(&s).await;
        let p = request(&s).await;
        let task = launch(s.clone(), token, p);
        let prompt = rx.recv().await.unwrap();
        match change {
            "lock" => s.session.lock().await.lock(),
            "revoke" => {
                s.clients.lock().await.revoke(&id);
            }
            "chain" => s.session.lock().await.set_chain(ChainId::Mainnet),
            "nonce" => n.nonce.store(1, Ordering::SeqCst),
            "replace" => *s.session.lock().await = session(),
            "nodechain" => n.wrong_chain.store(true, Ordering::SeqCst),
            _ => unreachable!(),
        };
        prompt.respond.send(Decision::Approve).unwrap();
        let r = task.await.unwrap();
        assert_eq!(r["status"], "cancelled", "{change}");
        assert!(r["transactionHash"].is_null());
        assert_eq!(n.broadcasts.load(Ordering::SeqCst), 0);
    }
}
#[tokio::test]
async fn concurrent_account_and_nonce_reservations_reject_second_intent() {
    let t = Temp::new();
    let (a, mut rx) = ChannelApprover::new(2);
    let n = Arc::new(Node::default());
    let s = make(&t, Arc::new(a), n.clone());
    let (_, token) = pair(&s).await;
    let p = request(&s).await;
    let task = launch(s.clone(), token.clone(), p.clone());
    let prompt = rx.recv().await.unwrap();
    let duplicate = transfer(&s, &token, p.clone()).await;
    assert_eq!(duplicate["status"], "awaiting_approval");
    let mut second = p;
    second["operationId"] = json!("12345678-1234-1234-1234-123456789abd");
    second["nonce"] = json!("0x1");
    let cancelled = transfer(&s, &token, second.clone()).await;
    assert_eq!(cancelled["status"], "cancelled");
    assert!(cancelled["transactionHash"].is_null());
    assert_eq!(transfer(&s, &token, second).await, cancelled);
    assert!(rx.try_recv().is_err());
    prompt.respond.send(Decision::Reject).unwrap();
    assert_eq!(task.await.unwrap()["status"], "rejected");
    assert_eq!(n.broadcasts.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn lost_response_and_wrong_hash_are_uncertain_and_cannot_retry_after_restart() {
    for wrong_hash in [false, true] {
        let t = Temp::new();
        let n = Arc::new(Node::default());
        n.ambiguous.store(!wrong_hash, Ordering::SeqCst);
        n.wrong_hash.store(wrong_hash, Ordering::SeqCst);
        let s = make(&t, Arc::new(AutoApprover(Decision::Approve)), n.clone());
        let (_, token) = pair(&s).await;
        let p = request(&s).await;
        let r = transfer(&s, &token, p.clone()).await;
        assert_eq!(r["status"], "unknown");
        assert!(r["transactionHash"].is_string());
        assert!(!r.to_string().contains("credential-canary"));
        drop(s);
        let reopened = make(&t, Arc::new(AutoApprover(Decision::Approve)), n.clone());
        assert_eq!(transfer(&reopened, &token, p.clone()).await, r);
        let mut other = p;
        other["operationId"] = json!("12345678-1234-1234-1234-123456789abd");
        assert_eq!(
            transfer(&reopened, &token, other).await["status"],
            "cancelled"
        );
        assert_eq!(n.broadcasts.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn schema_fee_mainnet_and_caller_scope_fail_before_approval() {
    let t = Temp::new();
    let n = Arc::new(Node::default());
    let s = make(&t, Arc::new(AutoApprover(Decision::Approve)), n.clone());
    let (_, token) = pair(&s).await;
    let p = request(&s).await;
    let mut cases = Vec::new();
    for (key, v) in [
        ("chainId", json!("0x534e5f4d41494e")),
        ("max_fee", json!("8")),
        ("submit", json!(true)),
        ("nonce", json!("0x00")),
        ("expires_at", json!(wallet_rpc::now_unix_ms() + 600001)),
    ] {
        let mut x = p.clone();
        x[key] = v;
        cases.push(x);
    }
    let mut x = p.clone();
    x["resource_bounds"]["tip"] = json!(1);
    cases.push(x);
    let mut x = p.clone();
    x["transfers"][0]["amount_low"] = json!("0x100000000000000000000000000000000");
    cases.push(x);
    for x in cases {
        assert!(rpc(&s, &token, "companion_treasuryTransfer", x)
            .await
            .error
            .is_some());
    }
    let (_, agent) = s
        .clients
        .lock()
        .await
        .pair("agent", wallet_rpc::ClientKind::Agent);
    assert!(rpc(&s, &agent, "companion_treasuryTransfer", p.clone())
        .await
        .error
        .is_some());
    assert_eq!(n.broadcasts.load(Ordering::SeqCst), 0);
    let caps = rpc(&s, "", "companion_treasuryCapabilities", json!({}))
        .await
        .result
        .unwrap();
    assert_eq!(caps["mainnetEnabled"], false);
    assert_eq!(caps["signingEnabled"], true);
}
#[tokio::test(start_paused = true)]
async fn approval_timeout_never_signs_and_late_approval_is_dead() {
    let t = Temp::new();
    let (a, mut rx) = ChannelApprover::new(1);
    let n = Arc::new(Node::default());
    let s = make(&t, Arc::new(a), n.clone());
    let (_, token) = pair(&s).await;
    let task = launch(s.clone(), token, request(&s).await);
    let prompt = rx.recv().await.unwrap();
    tokio::time::advance(std::time::Duration::from_secs(61)).await;
    assert_eq!(task.await.unwrap()["status"], "rejected");
    assert!(prompt.respond.send(Decision::Approve).is_err());
    assert_eq!(n.broadcasts.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn interrupted_prompt_persists_without_replay_and_status_is_owner_scoped() {
    let t = Temp::new();
    let (a, mut rx) = ChannelApprover::new(1);
    let n = Arc::new(Node::default());
    let s = make(&t, Arc::new(a), n.clone());
    let (_, token) = pair(&s).await;
    let p = request(&s).await;
    let task = launch(s.clone(), token.clone(), p.clone());
    let _prompt = rx.recv().await.unwrap();
    task.abort();
    let _ = task.await;
    drop(s);
    let reopened = make(&t, Arc::new(AutoApprover(Decision::Approve)), n.clone());
    assert_eq!(
        transfer(&reopened, &token, p.clone()).await["status"],
        "awaiting_approval"
    );
    let (_, other) = pair(&reopened).await;
    assert!(rpc(
        &reopened,
        &other,
        "companion_treasuryStatus",
        json!({"operationId":p["operationId"]})
    )
    .await
    .error
    .is_some());
    assert_eq!(n.broadcasts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn interrupted_unsigned_review_expires_but_signing_marker_never_releases() {
    let t = Temp::new();
    let (a, mut rx) = ChannelApprover::new(1);
    let n = Arc::new(Node::default());
    let s = make(&t, Arc::new(a), n.clone());
    let (_, token) = pair(&s).await;
    let mut p = request(&s).await;
    p["expires_at"] = json!(wallet_rpc::now_unix_ms() + 250);
    let task = launch(s.clone(), token.clone(), p.clone());
    let _prompt = rx.recv().await.unwrap();
    task.abort();
    let _ = task.await;
    tokio::time::sleep(std::time::Duration::from_millis(270)).await;
    let expired = rpc(
        &s,
        &token,
        "companion_treasuryStatus",
        json!({"operationId":p["operationId"]}),
    )
    .await
    .result
    .unwrap();
    assert_eq!(expired["status"], "expired");
    assert!(expired["transactionHash"].is_null());
    // Simulate a process dying after the durable pre-sign marker. Expiry MUST NOT release it.
    let db = rusqlite::Connection::open(t.0.join("treasury.sqlite3")).unwrap();
    db.execute("UPDATE treasury_ops SET status='submitting'", [])
        .unwrap();
    let uncertain = rpc(
        &s,
        &token,
        "companion_treasuryStatus",
        json!({"operationId":p["operationId"]}),
    )
    .await
    .result
    .unwrap();
    assert_eq!(uncertain["status"], "submitting");
    assert_eq!(transfer(&s, &token, p).await, uncertain);
    assert_eq!(n.broadcasts.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn missing_node_or_locked_account_get_bound_no_signature_cancellation() {
    for locked in [false, true] {
        let t = Temp::new();
        let s = make(
            &t,
            Arc::new(AutoApprover(Decision::Approve)),
            Arc::new(Node::default()),
        );
        let (_, token) = pair(&s).await;
        let p = request(&s).await;
        if locked {
            s.session.lock().await.lock();
        } else {
            s.set_node(ChainId::Sepolia, None);
        }
        let r = transfer(&s, &token, p.clone()).await;
        assert_eq!(r["status"], "cancelled");
        assert_eq!(r["plan_hash"], p["plan_hash"]);
        assert!(r["transactionHash"].is_null());
    }
}

#[tokio::test]
async fn accepted_recovery_unblocks_next_nonce_but_never_replays_or_reuses_consumed_nonce() {
    let t = Temp::new();
    let n = Arc::new(Node::default());
    n.ambiguous.store(true, Ordering::SeqCst);
    let s = make(&t, Arc::new(AutoApprover(Decision::Approve)), n.clone());
    let (_, token) = pair(&s).await;
    let p = request(&s).await;
    let first = transfer(&s, &token, p.clone()).await;
    assert_eq!(first["status"], "unknown");
    let q = json!({"operationId":p["operationId"]});
    assert_eq!(
        rpc(&s, &token, "companion_treasuryStatus", q.clone())
            .await
            .result
            .unwrap()["status"],
        "unknown"
    );
    let mut second = p.clone();
    second["operationId"] = json!("12345678-1234-1234-1234-123456789abd");
    second["nonce"] = json!("0x1");
    let cancelled = transfer(&s, &token, second.clone()).await;
    assert_eq!(cancelled["status"], "cancelled");
    assert!(cancelled["transactionHash"].is_null());
    n.accepted.store(true, Ordering::SeqCst);
    n.wrong_chain.store(true, Ordering::SeqCst);
    assert_eq!(
        rpc(&s, &token, "companion_treasuryStatus", q.clone())
            .await
            .result
            .unwrap()["status"],
        "unknown"
    );
    n.wrong_chain.store(false, Ordering::SeqCst);
    let (a, b) = tokio::join!(
        rpc(&s, &token, "companion_treasuryStatus", q.clone()),
        rpc(&s, &token, "companion_treasuryStatus", q)
    );
    for r in [a, b] {
        let r = r.result.unwrap();
        assert_eq!(r["status"], "submitted");
        assert_eq!(r["transactionHash"], first["transactionHash"]);
    }
    drop(s);
    let s = make(&t, Arc::new(AutoApprover(Decision::Approve)), n.clone());
    assert_eq!(transfer(&s, &token, second.clone()).await, cancelled);
    let mut old = p;
    old["operationId"] = json!("12345678-1234-1234-1234-123456789abe");
    assert_eq!(transfer(&s, &token, old).await["status"], "cancelled");
    n.ambiguous.store(false, Ordering::SeqCst);
    n.nonce.store(1, Ordering::SeqCst);
    second["operationId"] = json!("12345678-1234-1234-1234-123456789abf");
    assert_eq!(transfer(&s, &token, second).await["status"], "submitted");
    assert_eq!(n.broadcasts.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn accepted_status_racing_broadcast_completion_keeps_resolution_and_never_replays() {
    let t = Temp::new();
    let n = Arc::new(Node::default());
    n.hold_broadcast.store(true, Ordering::SeqCst);
    n.ambiguous.store(true, Ordering::SeqCst);
    let s = make(&t, Arc::new(AutoApprover(Decision::Approve)), n.clone());
    let (_, token) = pair(&s).await;
    let p = request(&s).await;
    let (s2, t2, p2) = (s.clone(), token.clone(), p.clone());
    let task = tokio::spawn(async move { rpc(&s2, &t2, "companion_treasuryTransfer", p2).await });
    n.broadcast_entered.notified().await;
    n.accepted.store(true, Ordering::SeqCst);
    let r = rpc(
        &s,
        &token,
        "companion_treasuryStatus",
        json!({"operationId":p["operationId"]}),
    )
    .await
    .result
    .unwrap();
    assert_eq!(r["status"], "submitted");
    n.broadcast_release.notify_one();
    assert!(task.await.unwrap().error.is_some()); // Original completion lost CAS; recover the committed state.
    assert_eq!(transfer(&s, &token, p).await, r);
    assert_eq!(n.broadcasts.load(Ordering::SeqCst), 1);
}
