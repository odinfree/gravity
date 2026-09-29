//! STRK20 wallet orchestration. Private SDK child over anonymous pipes; no keys
//! or private payloads on the loopback API. Only reviewed final calls broadcast.
use crate::{
    approval::{ApprovalRequest, Decision},
    dispatch::{self, ServerState},
    error::WalletRpcError,
    node::FeeBounds,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, path::PathBuf, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{Mutex, RwLock},
};
use wallet_core::{AccountRef, ChainId, Felt, InvokeV3Params};

const MAIN_POOL: &str = "0x040337b1af3c663e86e333bab5a4b28da8d4652a15a69beee2b677776ffe812a";
const TEST_POOL: &str = "0x0254a6b2997ef52e9f830ce1f543f6b29768295e8d17e2267d672c552cfe0d91";
const FRAME_LIMIT: usize = 32 * 1024 * 1024;
const SCREENED_RELAY: &str = "http://127.0.0.1:3001";
fn err(s: &str) -> WalletRpcError {
    WalletRpcError::Precondition(s.into())
}
fn felt(s: &str) -> Result<Felt, WalletRpcError> {
    (if s.starts_with("0x") {
        Felt::from_hex(s)
    } else {
        Felt::from_dec_str(s)
    })
    .map_err(|_| err("Invalid privacy field"))
}
fn fh(f: &Felt) -> String {
    format!("{f:#x}")
}
fn integer(s: &str) -> Result<u128, WalletRpcError> {
    if let Some(s) = s.strip_prefix("0x") {
        u128::from_str_radix(s, 16)
    } else {
        s.parse()
    }
    .map_err(|_| err("Privacy amount out of range"))
}
fn strk(n: u128) -> String {
    format!(
        "{}.{:018}",
        n / 1_000_000_000_000_000_000,
        n % 1_000_000_000_000_000_000
    )
}

#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScreeningPolicy {
    #[default]
    Required,
    PoolEnforced,
}
#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DepositProver {
    #[default]
    Configured,
    Starkscan,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkSettings {
    pub pool_address: String,
    pub discovery_url: String,
    #[serde(default)]
    pub screening_policy: ScreeningPolicy,
    #[serde(default)]
    pub deposit_prover: DepositProver,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub mainnet: NetworkSettings,
    pub testnet: NetworkSettings,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            mainnet: NetworkSettings {
                pool_address: MAIN_POOL.into(),
                discovery_url: "http://127.0.0.1:8080".into(),
                screening_policy: ScreeningPolicy::Required,
                deposit_prover: DepositProver::Configured,
            },
            testnet: NetworkSettings {
                pool_address: TEST_POOL.into(),
                discovery_url: String::new(),
                screening_policy: ScreeningPolicy::Required,
                deposit_prover: DepositProver::Configured,
            },
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), WalletRpcError> {
        if self.testnet.deposit_prover == DepositProver::Starkscan
            || (self.mainnet.deposit_prover == DepositProver::Starkscan
                && felt(&self.mainnet.pool_address)? != felt(MAIN_POOL)?)
        {
            return Err(err(
                "Starkscan shielding supports the current mainnet STRK20 pool only",
            ));
        }
        for n in [&self.mainnet, &self.testnet] {
            if n.screening_policy != ScreeningPolicy::Required
                && [felt(MAIN_POOL)?, felt(TEST_POOL)?].contains(&felt(&n.pool_address)?)
            {
                return Err(err("This STRK20 deployment requires deposit screening"));
            }
            if felt(&n.pool_address)? == Felt::ZERO {
                return Err(err("Pool must be nonzero"));
            }
            if !n.discovery_url.is_empty() {
                validate_endpoint(&n.discovery_url)?;
            }
        }
        Ok(())
    }
    fn network(&self, chain: ChainId) -> &NetworkSettings {
        match chain {
            ChainId::Mainnet => &self.mainnet,
            ChainId::Sepolia => &self.testnet,
        }
    }
}
pub fn validate_endpoint(s: &str) -> Result<(), WalletRpcError> {
    let u = reqwest::Url::parse(s).map_err(|_| err("Invalid privacy service URL"))?;
    let loopback = matches!(
        u.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    if !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
        || !(u.scheme() == "https" || (u.scheme() == "http" && loopback))
    {
        return Err(err(
            "Privacy services require HTTPS or loopback HTTP via an SSH tunnel",
        ));
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub account: String,
    pub chain_id: String,
    pub mode: String,
    #[serde(default)]
    pub operation: String,
    #[serde(default)]
    pub amount: String,
    #[serde(default)]
    pub recipient: String,
}
impl Request {
    fn chain(&self) -> Result<ChainId, WalletRpcError> {
        ChainId::from_felt(&felt(&self.chain_id)?).map_err(|_| err("Unsupported privacy network"))
    }
    fn validate(&self) -> Result<(), WalletRpcError> {
        self.chain()?;
        felt(&self.account)?;
        if !["status", "balances", "prepare"].contains(&self.mode.as_str()) {
            return Err(err("Invalid privacy mode"));
        }
        if self.mode == "prepare" {
            if !["register", "deposit", "transfer", "withdraw"].contains(&self.operation.as_str()) {
                return Err(err("Unsupported privacy action"));
            }
            if self.operation != "register"
                && (self.amount.starts_with("0x") || integer(&self.amount)? == 0)
            {
                return Err(err("Enter a positive STRK amount in base units"));
            }
            if ["transfer", "withdraw"].contains(&self.operation.as_str())
                && felt(&self.recipient)? == Felt::ZERO
            {
                return Err(err("Enter a recipient"));
            }
        }
        Ok(())
    }
}

struct Prepared {
    owner: String,
    account: AccountRef,
    chain: ChainId,
    epoch: u64,
    created: u64,
    request: Request,
    result: Value,
    config: Value,
}
pub struct PrivacyState {
    settings: RwLock<Settings>,
    path: PathBuf,
    worker: PathBuf,
    node: PathBuf,
    prepared: Mutex<HashMap<String, Prepared>>,
    operation: Mutex<()>,
}
impl PrivacyState {
    pub fn new(path: PathBuf, worker: PathBuf, node: PathBuf) -> Self {
        let settings = std::fs::read(path.join("settings.json"))
            .ok()
            .and_then(|s| serde_json::from_slice(&s).ok())
            .unwrap_or_default();
        Self {
            settings: RwLock::new(settings),
            path,
            worker,
            node,
            prepared: Mutex::new(HashMap::new()),
            operation: Mutex::new(()),
        }
    }
    pub async fn settings(&self) -> Settings {
        self.settings.read().await.clone()
    }
    pub async fn set_settings(&self, settings: Settings) -> Result<(), WalletRpcError> {
        settings.validate()?;
        write_private(&self.path.join("settings.json"), &json!(settings))?;
        *self.settings.write().await = settings;
        self.prepared.lock().await.clear();
        // Service operators can recheck after updating a backend at the same URL.
        write_private(&self.path.join("screening-observation.json"), &Value::Null)?;
        Ok(())
    }
    pub async fn clear(&self) {
        self.prepared.lock().await.clear();
    }
    pub fn runtime_ready(&self) -> bool {
        self.worker.is_file() && self.node.is_file()
    }
    fn screening_context(config: &Value) -> String {
        hex::encode(Sha256::digest(config.to_string().as_bytes()))
    }
    fn observe_screening(&self, config: &Value, missing: bool) -> Result<(), WalletRpcError> {
        write_private(
            &self.path.join("screening-observation.json"),
            &json!({
                "context_sha256":Self::screening_context(config), "missing":missing,
                "observed_at_ms":crate::now_unix_ms()
            }),
        )
    }
    fn deposit_screening(&self, config: &Value) -> &'static str {
        if config["screening_policy"] == "pool_enforced" {
            return "pool_enforced";
        }
        let observation: Value = std::fs::read(self.path.join("screening-observation.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(Value::Null);
        if observation["context_sha256"] == Self::screening_context(config)
            && observation["missing"] == true
        {
            "signature_missing"
        } else {
            // A health response or a previous signature cannot authorize a new deposit.
            "unverified"
        }
    }
}

fn write_private(path: &std::path::Path, value: &Value) -> Result<(), WalletRpcError> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| err("Privacy storage unavailable"))?;
    std::fs::create_dir_all(parent).map_err(|_| err("Cannot create privacy storage"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| err("Cannot protect privacy storage"))?;
    }
    let tmp = path.with_extension("tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts
        .open(&tmp)
        .map_err(|_| err("Cannot write privacy storage"))?;
    file.write_all(
        serde_json::to_string(value)
            .map_err(|_| err("Privacy serialization failed"))?
            .as_bytes(),
    )
    .map_err(|_| err("Cannot write privacy storage"))?;
    file.sync_all()
        .map_err(|_| err("Cannot sync privacy storage"))?;
    std::fs::rename(tmp, path).map_err(|_| err("Cannot save privacy storage"))
}

async fn configuration(state: &ServerState, chain: ChainId) -> Result<Value, WalletRpcError> {
    let privacy = state
        .privacy
        .as_ref()
        .ok_or_else(|| err("Privacy is not configured"))?;
    let p = state
        .prover
        .as_ref()
        .ok_or_else(|| err("Configure your prover first"))?;
    if p.prover.kind() != "starknet-rpc" {
        return Err(err("Privacy requires your starknet-rpc prover backend"));
    }
    let name = match chain {
        ChainId::Mainnet => "mainnet",
        ChainId::Sepolia => "testnet",
    };
    let net = p.settings.for_network(name).await;
    validate_endpoint(&net.rpc_url)?;
    validate_endpoint(&net.prover_url)?;
    let settings = privacy.settings().await;
    settings.validate()?;
    let private = settings.network(chain);
    validate_endpoint(&private.discovery_url)?;
    Ok(
        json!({"rpc_url":net.rpc_url,"prover_url":net.prover_url,"discovery_url":private.discovery_url,
        "adapter":"strk20-v2", "screening_policy":private.screening_policy, "deposit_prover":private.deposit_prover,
        "pool_address":private.pool_address,"chain_id":fh(&chain.as_felt())}),
    )
}

async fn account(
    state: &ServerState,
    owner: Option<&str>,
    address: &str,
) -> Result<AccountRef, WalletRpcError> {
    let normalized = wallet_core::address_hex(&felt(address)?);
    let session = state.session.lock().await;
    let found = session
        .registry()?
        .scoped_for(owner)
        .find(|a| a.address == normalized)
        .cloned()
        .ok_or(WalletRpcError::Forbidden);
    found
}

/// Safe, coarse stages only: never stream private inputs or provider errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressStage {
    Preparing,
    Proving,
    CheckingFees,
    Submitting,
}
pub type ProgressReporter = dyn Fn(ProgressStage) + Send + Sync;
fn report(progress: Option<&ProgressReporter>, stage: ProgressStage) {
    if let Some(progress) = progress {
        progress(stage);
    }
}

/// Native UI uses owner="desktop"/scope=None. RPC always supplies verified client scope.
pub async fn run(
    state: &ServerState,
    owner: &str,
    scope: Option<&str>,
    req: Request,
    prompt: bool,
) -> Result<Value, WalletRpcError> {
    run_with_progress(state, owner, scope, req, prompt, None).await
}
async fn run_with_progress(
    state: &ServerState,
    owner: &str,
    scope: Option<&str>,
    req: Request,
    prompt: bool,
    progress: Option<&ProgressReporter>,
) -> Result<Value, WalletRpcError> {
    req.validate()?;
    let privacy = state
        .privacy
        .as_ref()
        .ok_or_else(|| err("Privacy is not configured"))?;
    let _guard = privacy
        .operation
        .try_lock()
        .map_err(|_| err("A privacy operation is already running"))?;
    let account = account(state, scope, &req.account).await?;
    let chain = req.chain()?;
    let config = configuration(state, chain).await?;
    if uses_starkscan(&req, &config) {
        let relay = relay_status().await;
        if relay["reachable"] != true {
            return Err(err(
                "Start the shared Starkscan deposit adapter on 127.0.0.1:3001 before shielding",
            ));
        }
        if relay["local_remaining"] == 0 {
            return Err(err(
                "The shared hosted-proving allowance is exhausted for this UTC day",
            ));
        }
        if relay["retry_after_seconds"].as_u64().unwrap_or(0) > 0
            || relay["pending"].as_array().is_some_and(|a| !a.is_empty())
        {
            return Err(err("A hosted proof is unresolved or Starkscan requested backoff. Check the shared relay status before shielding again"));
        }
    }
    if req.mode == "prepare" {
        let records = history(state, scope, &account.address, chain).await?;
        if records.iter().any(|v| {
            matches!(
                v["status"].as_str(),
                Some("submission_pending" | "submitted" | "submission_unknown")
            )
        }) {
            return Err(err("An earlier privacy transaction needs a receipt check before preparing another. Open Recent privacy transactions."));
        }
        if let Some(last) = records
            .iter()
            .filter_map(|v| v["block_number"].as_u64())
            .max()
        {
            let url = config["rpc_url"]
                .as_str()
                .ok_or_else(|| err("Missing RPC"))?;
            let head = node_read(url, "starknet_blockNumber", json!([]))
                .await?
                .as_u64()
                .ok_or_else(|| err("Missing chain head"))?;
            if head.saturating_sub(10) <= last {
                return Err(err("The previous transaction is still maturing. Wait several blocks and prepare again."));
            }
        }
    }
    if prompt && req.mode != "status" {
        let summary = if req.mode == "balances" {
            "Read this account's shielded STRK balance".into()
        } else {
            format!("Prepare {} of {} STRK for {} on {}. Proving uses your configured private services; this step does not broadcast.", req.operation, strk(integer(if req.amount.is_empty() {"0"} else {&req.amount})?), account.address, req.chain_id)
        };
        if state
            .approver
            .request_approval(ApprovalRequest {
                client_label: owner.into(),
                method: "companion_privacyPrepare".into(),
                summary,
            })
            .await
            == Decision::Reject
        {
            return Err(WalletRpcError::UserRefused);
        }
    }
    let epoch = state.session.lock().await.epoch();
    let mut result = worker(state, &account, chain, &req, &config, progress).await?;
    if state.session.lock().await.epoch() != epoch {
        return Err(err("Wallet session changed; prepare again"));
    }
    if req.mode != "prepare" {
        result["deposit_screening"] = json!(privacy.deposit_screening(&config));
        if config["deposit_prover"] == "starkscan" {
            result["hosted_prover"] = relay_status().await;
        }
        return Ok(result);
    }
    validate_prepared(&req, &config, &result)?;
    screening_fresh(&req, &result, crate::now_unix_ms() / 1000)?;
    if req.operation == "deposit" {
        privacy.observe_screening(&config, false)?;
    }
    let mut random = [0u8; 16];
    getrandom::getrandom(&mut random).map_err(|_| err("Cannot create review identifier"))?;
    let id = hex::encode(random);
    let review = review(&id, &result);
    let mut entries = privacy.prepared.lock().await;
    entries.retain(|_, p| crate::now_unix_ms().saturating_sub(p.created) < 300_000);
    if entries.len() >= 8 {
        return Err(err(
            "Too many pending privacy reviews; cancel an old review",
        ));
    }
    entries.insert(
        id,
        Prepared {
            owner: owner.into(),
            account,
            chain,
            epoch,
            created: crate::now_unix_ms(),
            request: req,
            result,
            config,
        },
    );
    Ok(review)
}

fn review(id: &str, result: &Value) -> Value {
    let mut value = json!({"review_id":id,"expires_in_seconds":300});
    for key in [
        "operation",
        "account",
        "recipient",
        "amount",
        "token",
        "pool_address",
        "pool_fee",
        "max_network_fee",
        "proof_base",
        "chain_id",
        "warnings",
        "screening_attached",
        "screening_issued_at",
        "screening_policy",
        "adapter",
    ] {
        value[key] = result[key].clone();
    }
    value
}

pub fn validate_prepared(
    req: &Request,
    config: &Value,
    result: &Value,
) -> Result<(), WalletRpcError> {
    let pool = felt(
        config["pool_address"]
            .as_str()
            .ok_or_else(|| err("Missing pool"))?,
    )?;
    let token = felt(dispatch::STRK_TOKEN_ADDRESS)?;
    let read_felt = |key: &str| -> Result<Felt, WalletRpcError> {
        felt(
            result[key]
                .as_str()
                .ok_or_else(|| err("Missing review field"))?,
        )
    };
    let recipient = if req.recipient.is_empty() {
        &req.account
    } else {
        &req.recipient
    };
    if read_felt("chain_id")? != req.chain()?.as_felt()
        || read_felt("pool_address")? != pool
        || read_felt("token")? != token
        || read_felt("recipient")? != felt(recipient)?
    {
        return Err(err("Privacy review context changed"));
    }
    if result["operation"] != req.operation
        || felt(
            result["account"]
                .as_str()
                .ok_or_else(|| err("Missing account"))?,
        )? != felt(&req.account)?
    {
        return Err(err("Privacy review does not match the request"));
    }
    let quantity = if req.operation == "register" {
        0
    } else {
        integer(&req.amount)?
    };
    if integer(
        result["amount"]
            .as_str()
            .ok_or_else(|| err("Missing amount"))?,
    )? != quantity
    {
        return Err(err("Privacy amount changed"));
    }
    let fee = integer(
        result["pool_fee"]
            .as_str()
            .ok_or_else(|| err("Missing pool fee"))?,
    )?;
    let expected = fee
        .checked_add(if req.operation == "deposit" {
            quantity
        } else {
            0
        })
        .ok_or_else(|| err("Approval overflow"))?;
    let calls = dispatch::parse_calls(result)?;
    let offset = usize::from(expected > 0);
    if calls.len() != offset + 1 {
        return Err(err("Unexpected privacy calls"));
    }
    if expected > 0
        && (calls[0].to != token
            || calls[0].selector != wallet_core::get_selector_from_name("approve")
            || calls[0].calldata != vec![pool, Felt::from(expected), Felt::ZERO])
    {
        return Err(err(
            "Privacy approval differs from the exact reviewed amount",
        ));
    }
    let apply = &calls[offset];
    if apply.to != pool
        || apply.selector != wallet_core::get_selector_from_name("apply_actions")
        || apply.calldata.is_empty()
    {
        return Err(err("Unexpected pool submission"));
    }
    if req.operation == "deposit"
        && config["screening_policy"] != "pool_enforced"
        && result["screening_attached"] != true
    {
        return Err(err("Screening signature required before shielding"));
    }
    if result["proof"].as_str().is_none_or(str::is_empty)
        || result["proof_facts"].as_array().is_none_or(Vec::is_empty)
    {
        return Err(err("Missing pool proof"));
    }
    let bounds =
        dispatch::opt_fee_bounds(result)?.ok_or_else(|| err("Missing network fee bounds"))?;
    let cap = fee_cap(&bounds)?;
    if cap == 0
        || cap
            != integer(
                result["max_network_fee"]
                    .as_str()
                    .ok_or_else(|| err("Missing maximum fee"))?,
            )?
    {
        return Err(err("Invalid network fee cap"));
    }
    Ok(())
}
fn fee_cap(bounds: &FeeBounds) -> Result<u128, WalletRpcError> {
    [&bounds.l1_gas, &bounds.l2_gas, &bounds.l1_data_gas]
        .iter()
        .try_fold(0u128, |sum, b| {
            (b.max_amount as u128)
                .checked_mul(b.max_price_per_unit)
                .and_then(|v| sum.checked_add(v))
                .ok_or_else(|| err("Network fee overflow"))
        })
}

async fn worker(
    state: &ServerState,
    account: &AccountRef,
    chain: ChainId,
    request: &Request,
    config: &Value,
    progress: Option<&ProgressReporter>,
) -> Result<Value, WalletRpcError> {
    let privacy = state
        .privacy
        .as_ref()
        .ok_or_else(|| err("Privacy unavailable"))?;
    if !privacy.runtime_ready() {
        return Err(err(
            "Build the privacy worker and install Node.js 24 or newer",
        ));
    }
    let pool = felt(
        config["pool_address"]
            .as_str()
            .ok_or_else(|| err("Missing pool"))?,
    )?;
    let viewing_key = state
        .session
        .lock()
        .await
        .privacy_viewing_key(account, chain, &pool)?;
    let epoch = state.session.lock().await.epoch();
    let mut input = serde_json::to_value(request).map_err(|_| err("Invalid privacy request"))?;
    input["config"] = config.clone();
    input["viewing_key"] = json!(viewing_key.as_str());
    if request.recipient.is_empty() {
        input["recipient"] = json!(account.address);
    }
    let mut command = Command::new(&privacy.node);
    command
        .arg(&privacy.worker)
        .env_clear()
        .env("NODE_ENV", "production")
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|_| err("Cannot start the privacy worker"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| err("Worker input unavailable"))?;
    let mut stdout = BufReader::new(
        child
            .stdout
            .take()
            .ok_or_else(|| err("Worker output unavailable"))?,
    );
    let initial = zeroize::Zeroizing::new(input.to_string() + "\n");
    input["viewing_key"] = Value::Null;
    stdin
        .write_all(initial.as_bytes())
        .await
        .map_err(|_| err("Cannot initialize privacy worker"))?;
    let process = async {
        let mut expected_tx: Option<Value> = None;
        let mut proved = false;
        let mut screening_issued_at = Value::Null;
        loop {
            let frame = read_frame(&mut stdout).await?;
            match frame["kind"].as_str() {
                Some("sign") => {
                    if request.mode != "prepare" || expected_tx.is_some() {
                        return Err(err("Unexpected privacy signing request"));
                    }
                    let p = &frame["payload"];
                    let details = &p["details"];
                    let calls = dispatch::parse_calls(&json!({"calls":p["calls"]}))?;
                    let key_felt = felt(&viewing_key)?;
                    if calls.len() != 1
                        || calls[0].to != pool
                        || calls[0].selector
                            != wallet_core::get_selector_from_name("compile_actions")
                        || calls[0].calldata.len() < 3
                        || calls[0].calldata[0] != felt(&account.address)?
                        || calls[0].calldata[1] != key_felt
                        || felt(
                            details["walletAddress"]
                                .as_str()
                                .ok_or_else(|| err("Missing virtual sender"))?,
                        )? != pool
                        || felt(
                            details["chainId"]
                                .as_str()
                                .ok_or_else(|| err("Missing virtual chain"))?,
                        )? != chain.as_felt()
                        || details["version"] != "0x3"
                        || details["tip"].as_str() != Some("0")
                    {
                        return Err(err("Privacy worker requested an out-of-scope signature"));
                    }
                    let bounds = dispatch::opt_fee_bounds(
                        &json!({"resource_bounds":details["resourceBounds"]}),
                    )?
                    .ok_or_else(|| err("Missing virtual bounds"))?;
                    if fee_cap(&bounds)? != 0 {
                        return Err(err("Private proof invocation must have zero effective fee"));
                    }
                    let nonce = felt(
                        details["nonce"]
                            .as_str()
                            .ok_or_else(|| err("Missing virtual nonce"))?,
                    )?;
                    let params = InvokeV3Params {
                        nonce,
                        tip: 0,
                        l1_gas: bounds.l1_gas,
                        l2_gas: bounds.l2_gas,
                        l1_data_gas: bounds.l1_data_gas,
                        ..Default::default()
                    };
                    let session = state.session.lock().await;
                    if session.epoch() != epoch {
                        return Err(err("Wallet session changed"));
                    }
                    let signed =
                        session.sign_privacy_for(account, &pool, &calls, chain, &params)?;
                    expected_tx = Some(crate::node::invoke_v3_tx_json(
                        &pool,
                        &signed.calldata,
                        &[signed.r, signed.s],
                        &nonce,
                        &bounds,
                        &[],
                        None,
                    ));
                    let response = json!({"id":frame["id"],"result":[fh(&signed.r),fh(&signed.s)]});
                    stdin
                        .write_all((response.to_string() + "\n").as_bytes())
                        .await
                        .map_err(|_| err("Worker closed before signature"))?;
                }
                Some("prove") => {
                    if proved {
                        return Err(err("Duplicate proof request"));
                    }
                    let expected = expected_tx
                        .as_ref()
                        .ok_or_else(|| err("Unsigned proof request"))?;
                    let tx = &frame["payload"]["transaction"];
                    // Numeric felt encodings may be padded; compare canonically.
                    if canonical(tx) != canonical(expected) {
                        return Err(err(
                            "Proving transaction differs from the signed invocation",
                        ));
                    }
                    let base = frame["payload"]["block_number"]
                        .as_u64()
                        .ok_or_else(|| err("Missing proof base"))?;
                    report(progress, ProgressStage::Proving);
                    let result = if uses_starkscan(request, config) {
                        screened_proof(tx, base).await?
                    } else {
                        let prover = state.prover.as_ref().ok_or_else(|| err("No prover"))?;
                        let job = prover::enqueue_prove(
                            prover,
                            json!({"transaction":tx,"block_number":base}),
                            Some("STRK20 pool operation".into()),
                            match chain {
                                ChainId::Mainnet => "mainnet",
                                ChainId::Sepolia => "testnet",
                            }
                            .into(),
                        )
                        .await;
                        loop {
                            let job = prover
                                .jobs
                                .get(&job)
                                .await
                                .ok_or_else(|| err("Proof job disappeared"))?;
                            match job.status{
                            prover::JobStatus::Succeeded=>break job.result.ok_or_else(||err("Missing proof"))?,
                            prover::JobStatus::Failed=>return Err(err("Pool proving failed. Check your prover and screening service; private error details withheld.")),
                            _=>tokio::time::sleep(Duration::from_millis(500)).await,
                        }
                        }
                    };
                    report(progress, ProgressStage::CheckingFees);
                    screening_issued_at =
                        result["additional_data"]["signature"]["issued_at"].clone();
                    proved = true;
                    stdin
                        .write_all(
                            (json!({"id":frame["id"],"result":result}).to_string() + "\n")
                                .as_bytes(),
                        )
                        .await
                        .map_err(|_| err("Worker closed before proof"))?;
                }
                Some("result") => {
                    if request.mode == "prepare" && !proved {
                        return Err(err("Privacy result has no completed proof"));
                    }
                    let mut result = frame["result"].clone();
                    result["screening_issued_at"] = screening_issued_at;
                    return Ok(result);
                }
                Some("error") => {
                    // Accept only known codes; never forward arbitrary child errors or inputs.
                    if frame["code"] == "SCREENING_REQUIRED" && request.operation == "deposit" {
                        privacy.observe_screening(config, true)?;
                    }
                    let message=match frame["code"].as_str(){
                        Some("SCREENING_REQUIRED")=>"Shielding is unavailable for this pool: the prover returned no authorized screening signature. gravity needs a screening-provider integration. Nothing was submitted or spent.",
                        Some("DISCOVERY")=>"Shielded balance unavailable: the discovery service could not complete the request. Registration and public balance are unchanged.",
                        Some("KEY_MISMATCH")=>"This account is registered with a different viewing key. Restore its original privacy wallet.",
                        Some("REGISTER")=>"Register this account first and wait for registration to mature.",
                        Some("REGISTERED")=>"This account is already registered.",
                        Some("RECIPIENT")=>"The recipient must register in this pool first.",
                        Some("MATURITY")=>"Funding is insufficient or not yet mature at the proof base.",
                        Some("BALANCE")=>"Insufficient mature shielded STRK balance.",
                        Some("CHAIN")=>"The RPC node is on the wrong network.",
                        Some("NODE")=>"The node rejected a privacy read or fee estimate. Nothing was submitted.",
                        _=>"Privacy operation failed. Check the configured services; private details withheld.",
                    };
                    return Err(err(message));
                }
                _ => return Err(err("Invalid privacy worker response")),
            }
        }
    };
    let watch_session = async {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if state.session.lock().await.epoch() != epoch {
                break;
            }
        }
    };
    let result = tokio::select! {
        result=process => result,
        _=watch_session => Err(err("Wallet locked or session changed; privacy worker stopped")),
        _=tokio::time::sleep(Duration::from_secs(if request.mode == "prepare" {960} else {60})) => Err(err("Privacy worker timed out; no submission was attempted")),
    };
    let _ = child.kill().await;
    let _ = child.wait().await;
    result
}

fn canonical(value: &Value) -> Value {
    match value {
        Value::String(s) => felt(s)
            .map(|f| json!(fh(&f)))
            .unwrap_or_else(|_| value.clone()),
        Value::Array(a) => Value::Array(a.iter().map(canonical).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .filter(|(k, v)| {
                    !(*k == "account_deployment_data" && v.as_array().is_some_and(Vec::is_empty))
                })
                .map(|(k, v)| (k.clone(), canonical(v)))
                .collect(),
        ),
        _ => value.clone(),
    }
}
async fn read_frame<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Value, WalletRpcError> {
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    loop {
        let chunk = reader
            .fill_buf()
            .await
            .map_err(|_| err("Privacy worker read failed"))?;
        if chunk.is_empty() {
            return Err(err("Privacy worker exited without a result"));
        }
        let n = chunk
            .iter()
            .position(|b| *b == b'\n')
            .map_or(chunk.len(), |i| i + 1);
        if bytes.len() + n > FRAME_LIMIT {
            return Err(err("Privacy worker response too large"));
        }
        let ended = chunk[n - 1] == b'\n';
        bytes.extend_from_slice(&chunk[..n]);
        reader.consume(n);
        if ended {
            return serde_json::from_slice(&bytes).map_err(|_| err("Invalid privacy worker JSON"));
        }
    }
}

/// One native desktop click authorizes only this request and these fee ceilings.
/// This entry point is not exposed through the agent JSON-RPC dispatcher.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopLimits {
    pub max_pool_fee: String,
    pub max_network_fee: String,
}
impl DesktopLimits {
    fn validate(&self) -> Result<(), WalletRpcError> {
        integer(&self.max_pool_fee)?;
        if integer(&self.max_network_fee)? == 0 {
            return Err(err("Set a positive maximum network fee"));
        }
        Ok(())
    }
    fn check(&self, result: &Value) -> Result<(), WalletRpcError> {
        self.validate()?;
        for (name, ceiling) in [
            ("pool_fee", &self.max_pool_fee),
            ("max_network_fee", &self.max_network_fee),
        ] {
            if integer(
                result[name]
                    .as_str()
                    .ok_or_else(|| err("Missing prepared fee"))?,
            )? > integer(ceiling)?
            {
                return Err(err("Prepared fees exceed the limits shown when you clicked. Nothing was submitted; review the limits and try again."));
            }
        }
        Ok(())
    }
}
pub async fn execute_desktop(
    state: &ServerState,
    request: Request,
    limits: DesktopLimits,
) -> Result<Value, WalletRpcError> {
    execute_desktop_with_progress(state, request, limits, None).await
}
pub async fn execute_desktop_with_progress(
    state: &ServerState,
    request: Request,
    limits: DesktopLimits,
    progress: Option<&ProgressReporter>,
) -> Result<Value, WalletRpcError> {
    report(progress, ProgressStage::Preparing);
    limits.validate()?;
    if request.mode != "prepare" {
        return Err(err("Expected a privacy transaction"));
    }
    let review = run_with_progress(state, "desktop", None, request, false, progress).await?;
    let id = review["review_id"]
        .as_str()
        .ok_or_else(|| err("Missing prepared transaction"))?;
    submit_inner(state, "desktop", None, id, Some(&limits), progress).await
}

pub async fn submit(
    state: &ServerState,
    owner: &str,
    scope: Option<&str>,
    id: &str,
) -> Result<Value, WalletRpcError> {
    submit_inner(state, owner, scope, id, None, None).await
}
async fn submit_inner(
    state: &ServerState,
    owner: &str,
    scope: Option<&str>,
    id: &str,
    desktop_limits: Option<&DesktopLimits>,
    progress: Option<&ProgressReporter>,
) -> Result<Value, WalletRpcError> {
    let privacy = state
        .privacy
        .as_ref()
        .ok_or_else(|| err("Privacy unavailable"))?;
    let _guard = privacy
        .operation
        .try_lock()
        .map_err(|_| err("A privacy operation is already running"))?;
    let prepared = {
        let mut entries = privacy.prepared.lock().await;
        let p = entries
            .get(id)
            .ok_or_else(|| err("Review expired; prepare again"))?;
        if p.owner != owner {
            return Err(WalletRpcError::Forbidden);
        }
        entries
            .remove(id)
            .ok_or_else(|| err("Review already consumed"))?
    };
    account(state, scope, &prepared.account.address).await?;
    if crate::now_unix_ms().saturating_sub(prepared.created) > 300_000
        || state.session.lock().await.epoch() != prepared.epoch
    {
        return Err(err(
            "Review expired or wallet session changed; prepare again",
        ));
    }
    let config = configuration(state, prepared.chain).await?;
    if config != prepared.config {
        return Err(err("Privacy configuration changed; prepare again"));
    }
    validate_prepared(&prepared.request, &config, &prepared.result)?;
    screening_fresh(
        &prepared.request,
        &prepared.result,
        crate::now_unix_ms() / 1000,
    )?;
    let r = &prepared.result;
    let summary=format!("{} {} STRK on {}. From {}. Recipient {}. Pool fee {} STRK; maximum network fee {} STRK. Deposits/withdrawals and this submitting account remain public.",
        prepared.request.operation,strk(integer(r["amount"].as_str().ok_or_else(||err("Missing amount"))?)?),prepared.request.chain_id,prepared.account.address,r["recipient"].as_str().unwrap_or("self"),
        strk(integer(r["pool_fee"].as_str().ok_or_else(||err("Missing pool fee"))?)?),strk(integer(r["max_network_fee"].as_str().ok_or_else(||err("Missing network fee"))?)?));
    if let Some(limits) = desktop_limits {
        // Only execute_desktop can supply limits. Generic agent grants never
        // acquire this path, and all preparation/signing checks still run.
        if owner != "desktop" || scope.is_some() {
            return Err(WalletRpcError::Forbidden);
        }
        limits.check(r)?;
    } else if state
        .approver
        .request_approval(ApprovalRequest {
            client_label: owner.into(),
            method: "companion_privacySubmit".into(),
            summary,
        })
        .await
        == Decision::Reject
    {
        return Err(WalletRpcError::UserRefused);
    }
    if state.session.lock().await.epoch() != prepared.epoch
        || crate::now_unix_ms().saturating_sub(prepared.created) > 300_000
    {
        return Err(err("Privacy review expired while awaiting approval"));
    }
    if configuration(state, prepared.chain).await? != prepared.config {
        return Err(err(
            "Privacy configuration changed during approval; prepare again",
        ));
    }
    screening_fresh(
        &prepared.request,
        &prepared.result,
        crate::now_unix_ms() / 1000,
    )?;
    let rpc_url = config["rpc_url"]
        .as_str()
        .ok_or_else(|| err("Missing RPC"))?;
    let head = node_read(rpc_url, "starknet_blockNumber", json!([]))
        .await?
        .as_u64()
        .ok_or_else(|| err("Missing chain head"))?;
    let base = r["proof_base"]
        .as_u64()
        .ok_or_else(|| err("Missing proof base"))?;
    if head.saturating_sub(base) < 10 {
        return Err(err("Pool proof base is not mature"));
    }
    let pool = config["pool_address"]
        .as_str()
        .ok_or_else(|| err("Missing pool"))?;
    let validity=node_read(rpc_url,"starknet_call",json!({"block_id":"latest","request":{"contract_address":pool,"entry_point_selector":fh(&wallet_core::get_selector_from_name("get_proof_validity_blocks")),"calldata":[]}})).await?;
    let validity = integer(
        validity[0]
            .as_str()
            .ok_or_else(|| err("Missing pool proof validity"))?,
    )?;
    if (head - base) as u128 >= validity.saturating_sub(10) {
        return Err(err("Pool proof expired; prepare again"));
    }
    let current_fee=node_read(rpc_url,"starknet_call",json!({"block_id":"latest","request":{"contract_address":pool,"entry_point_selector":fh(&wallet_core::get_selector_from_name("get_fee_amount")),"calldata":[]}})).await?;
    if integer(
        current_fee[0]
            .as_str()
            .ok_or_else(|| err("Missing current pool fee"))?,
    )? != integer(
        r["pool_fee"]
            .as_str()
            .ok_or_else(|| err("Missing reviewed fee"))?,
    )? {
        return Err(err("Pool fee changed since review; prepare again"));
    }
    let node = state
        .node_for(prepared.chain)
        .ok_or(WalletRpcError::NoNode)?;
    let sender = felt(&prepared.account.address)?;
    let nonce = felt(r["nonce"].as_str().ok_or_else(|| err("Missing nonce"))?)?;
    if node
        .get_nonce(&sender)
        .await
        .map_err(|_| err("Cannot verify current account nonce"))?
        != nonce
    {
        return Err(err("Account changed after review; prepare again"));
    }
    let calls = dispatch::parse_calls(r)?;
    screening_fresh(&prepared.request, r, crate::now_unix_ms() / 1000)?;
    if prepared.request.operation == "deposit" && r["screening_attached"] == true {
        let block = node_read(
            rpc_url,
            "starknet_getBlockWithTxHashes",
            json!({"block_id":"latest"}),
        )
        .await?;
        let timestamp = block["timestamp"]
            .as_u64()
            .ok_or_else(|| err("Cannot verify screening against the chain clock"))?;
        screening_fresh(
            &prepared.request,
            r,
            timestamp.max(crate::now_unix_ms() / 1000),
        )?;
    }
    let bounds = dispatch::opt_fee_bounds(r)?.ok_or_else(|| err("Missing bounds"))?;
    let proof_facts = r["proof_facts"]
        .as_array()
        .ok_or_else(|| err("Missing proof facts"))?
        .iter()
        .map(|v| felt(v.as_str().ok_or_else(|| err("Invalid proof fact"))?))
        .collect::<Result<Vec<_>, _>>()?;
    let proof = r["proof"].as_str().ok_or_else(|| err("Missing proof"))?;
    let params = InvokeV3Params {
        nonce,
        tip: 0,
        l1_gas: bounds.l1_gas,
        l2_gas: bounds.l2_gas,
        l1_data_gas: bounds.l1_data_gas,
        proof_facts: proof_facts.clone(),
        ..Default::default()
    };
    let signed = {
        let s = state.session.lock().await;
        if s.epoch() != prepared.epoch {
            return Err(err("Wallet session changed"));
        }
        s.sign_invoke_for(&prepared.account, &calls, prepared.chain, &params)?
    };
    let tx_hash = fh(&signed.transaction_hash);
    let journal = privacy.path.join(format!("transaction-{}.json", tx_hash));
    write_private(
        &journal,
        &json!({"transaction_hash":tx_hash,"chain_id":prepared.request.chain_id,"account":prepared.account.address,"status":"submission_pending"}),
    )?;
    report(progress, ProgressStage::Submitting);
    let submitted = node
        .add_invoke(
            &sender,
            &signed.calldata,
            &[signed.r, signed.s],
            &nonce,
            &bounds,
            &proof_facts,
            Some(proof),
        )
        .await;
    let outcome = match submitted {
        Ok(hash) if hash == signed.transaction_hash => "submitted",
        _ => "submission_unknown",
    };
    write_private(
        &journal,
        &json!({"transaction_hash":tx_hash,"chain_id":prepared.request.chain_id,"account":prepared.account.address,"status":outcome}),
    )?;
    // A network error never causes a second broadcast. Return the deterministic
    // hash and let receipt polling reconcile it after a restart as well.
    Ok(json!({"transaction_hash":tx_hash,"status":outcome,"chain_id":prepared.request.chain_id}))
}

pub async fn receipt(
    state: &ServerState,
    hash: &str,
    chain: ChainId,
) -> Result<Value, WalletRpcError> {
    felt(hash)?;
    let config = configuration(state, chain).await?;
    let value = node_read(
        config["rpc_url"]
            .as_str()
            .ok_or_else(|| err("Missing RPC"))?,
        "starknet_getTransactionReceipt",
        json!({"transaction_hash":hash}),
    )
    .await?;
    let result = json!({"transaction_hash":hash,"execution_status":value["execution_status"],"finality_status":value["finality_status"],"block_number":value["block_number"],"actual_fee":value["actual_fee"]});
    if let Some(p) = &state.privacy {
        let path = p
            .path
            .join(format!("transaction-{}.json", fh(&felt(hash)?)));
        if path.exists() {
            let mut saved: Value = serde_json::from_slice(
                &std::fs::read(&path).map_err(|_| err("Cannot read receipt journal"))?,
            )
            .map_err(|_| err("Invalid receipt journal"))?;
            for (k, v) in result.as_object().ok_or_else(|| err("Invalid receipt"))? {
                saved[k] = v.clone();
            }
            if matches!(
                value["finality_status"].as_str(),
                Some("ACCEPTED_ON_L2" | "ACCEPTED_ON_L1")
            ) {
                saved["status"] = value["execution_status"].clone();
            }
            write_private(&path, &saved)?;
        }
    }
    Ok(result)
}

pub async fn history(
    state: &ServerState,
    scope: Option<&str>,
    address: &str,
    chain: ChainId,
) -> Result<Vec<Value>, WalletRpcError> {
    let account = account(state, scope, address).await?;
    let privacy = state
        .privacy
        .as_ref()
        .ok_or_else(|| err("Privacy unavailable"))?;
    let dir = match std::fs::read_dir(&privacy.path) {
        Ok(dir) => dir,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(_) => return Err(err("Cannot read privacy history")),
    };
    let mut records = Vec::new();
    for file in dir.flatten() {
        let name = file.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("transaction-0x") || !name.ends_with(".json") {
            continue;
        }
        let bytes = std::fs::read(file.path()).map_err(|_| err("Cannot read privacy receipt"))?;
        if bytes.len() > 8192 {
            continue;
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| err("Invalid privacy receipt journal"))?;
        if value["account"].as_str().and_then(|s| felt(s).ok()) == Some(felt(&account.address)?)
            && value["chain_id"].as_str().and_then(|s| felt(s).ok()) == Some(chain.as_felt())
        {
            records.push(value);
        }
    }
    records.sort_by_key(|v| std::cmp::Reverse(v["block_number"].as_u64().unwrap_or(u64::MAX)));
    Ok(records)
}

fn uses_starkscan(request: &Request, config: &Value) -> bool {
    request.mode == "prepare"
        && request.operation == "deposit"
        && config["deposit_prover"] == "starkscan"
        && config["chain_id"] == "0x534e5f4d41494e"
}

fn screening_fresh(request: &Request, result: &Value, now: u64) -> Result<(), WalletRpcError> {
    if request.operation != "deposit" || result["screening_attached"] != true {
        return Ok(());
    }
    let value = &result["screening_issued_at"];
    let issued = value
        .as_u64()
        .or_else(|| {
            value
                .as_str()
                .and_then(|s| integer(s).ok())
                .and_then(|v| u64::try_from(v).ok())
        })
        .ok_or_else(|| err("Missing screening issue time; prepare again"))?;
    // The current pool allows 300 seconds. Reserve 60 for broadcast/inclusion.
    if issued == 0 || issued > now.saturating_add(30) || issued.saturating_add(240) <= now {
        return Err(err("Screening expired or has too little time remaining. Nothing was submitted; prepare a fresh deposit"));
    }
    Ok(())
}

async fn relay_status() -> Value {
    match node_read(SCREENED_RELAY, "gravity_relayStatus", json!([])).await {
        Ok(value)
            if value["service"] == "gravity-starkscan-relay"
                && value["chain_id"] == "0x534e5f4d41494e" =>
        {
            json!({"reachable":true,"local_attempts":value["local_attempts"],
                "local_limit":value["local_limit"],"local_remaining":value["local_remaining"],
                "resets_at":value["resets_at"],"retry_after_seconds":value["retry_after_seconds"],
                "pending":value["pending"]})
        }
        _ => json!({"reachable":false}),
    }
}

// The shared adapter owns the credential, quota and one-shot result persistence.
// No hosted API key crosses the frontend, SDK child, or companion RPC boundary.
async fn screened_proof(tx: &Value, block: u64) -> Result<Value, WalletRpcError> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(930))
        .build()
        .map_err(|_| err("Cannot initialize deposit prover"))?;
    let mut response = client.post(SCREENED_RELAY)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"starknet_proveTransaction",
            "params":{"block_id":{"block_number":block},"transaction":tx}}))
        .send().await.map_err(|_| err("Deposit prover disconnected. Check the shared relay status before trying another proof"))?;
    if !response.status().is_success() {
        return Err(err("The shared deposit prover returned an HTTP error"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| err("Deposit proof response interrupted"))?
    {
        if bytes.len().saturating_add(chunk.len()) > FRAME_LIMIT {
            return Err(err("Deposit proof response too large"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let body: Value =
        serde_json::from_slice(&bytes).map_err(|_| err("Invalid deposit prover response"))?;
    if body["id"] != 1 || body["jsonrpc"] != "2.0" {
        return Err(err("Invalid deposit prover envelope"));
    }
    if body.get("error").is_some() {
        return Err(err(match body["error"]["code"].as_i64() {
            Some(-32060) => "Starkscan has not enabled the hosted prover endpoint",
            Some(-32061) => "Starkscan rejected the key or its prove scope",
            Some(-32062) => "The hosted proving allowance is exhausted for this UTC day",
            Some(-32063) => "Another hosted proof is unresolved or the relay is backing off",
            Some(-32064) => "Hosted proof delivery is uncertain. Contact Starkscan using the job ID in the shared relay status; do not resubmit",
            Some(-32065) => "Hosted proof is still pending or interrupted. Check the shared relay status before preparing another",
            Some(-32066) => "Screening is missing or too close to expiry. Nothing was submitted",
            Some(-32067) => "Starkscan rejected the proof. Private diagnostics are in the relay's local storage",
            Some(-32068) => "Starkscan proving is unavailable. Check the shared relay before trying again",
            _ => "The deposit prover failed; private details withheld",
        }));
    }
    body.get("result")
        .cloned()
        .ok_or_else(|| err("Missing screened proof"))
}

async fn node_read(url: &str, method: &str, params: Value) -> Result<Value, WalletRpcError> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| err("Cannot initialize RPC"))?;
    let mut res = client
        .post(url)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await
        .map_err(|_| err("Node request failed; retry receipt lookup without resubmitting"))?;
    if !res.status().is_success() {
        return Err(err("Node returned an HTTP error"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = res
        .chunk()
        .await
        .map_err(|_| err("Invalid node response"))?
    {
        if bytes.len().saturating_add(chunk.len()) > 1024 * 1024 {
            return Err(err("Node response too large"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let body: Value = serde_json::from_slice(&bytes).map_err(|_| err("Invalid node response"))?;
    if body["error"]["code"] == 29 {
        return Err(err(
            "Receipt not available yet; keep the transaction hash and retry lookup",
        ));
    }
    if body.get("error").is_some() || body["id"] != 1 {
        return Err(err("Node rejected the request"));
    }
    body.get("result")
        .cloned()
        .ok_or_else(|| err("Empty node response"))
}

pub fn default_node_path() -> PathBuf {
    if let Some(path) = std::env::var_os("STRKD_PRIVACY_NODE") {
        return path.into();
    }
    [
        "/opt/homebrew/bin/node",
        "/usr/local/bin/node",
        "/usr/bin/node",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.is_file())
    .unwrap_or_else(|| PathBuf::from("node"))
}
pub fn attach(state: ServerState, path: PathBuf, worker: PathBuf) -> ServerState {
    state.with_privacy(Arc::new(PrivacyState::new(
        path,
        worker,
        default_node_path(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(operation: &str) -> Request {
        Request {
            account: "0x123".into(),
            chain_id: "0x534e5f4d41494e".into(),
            mode: "prepare".into(),
            operation: operation.into(),
            amount: "10".into(),
            recipient: "0x456".into(),
        }
    }
    #[test]
    fn starkscan_is_deposit_only_and_mainnet_only() {
        let config = json!({"deposit_prover":"starkscan","chain_id":"0x534e5f4d41494e"});
        assert!(uses_starkscan(&request("deposit"), &config));
        for op in ["register", "transfer", "withdraw"] {
            assert!(!uses_starkscan(&request(op), &config));
        }
        let mut read = request("deposit");
        read.mode = "balances".into();
        assert!(!uses_starkscan(&read, &config));
        let mut settings = Settings::default();
        settings.testnet.deposit_prover = DepositProver::Starkscan;
        assert!(settings.validate().is_err());
        settings.testnet.deposit_prover = DepositProver::Configured;
        settings.mainnet.deposit_prover = DepositProver::Starkscan;
        assert!(settings.validate().is_ok());
        settings.mainnet.pool_address = "0x123".into();
        assert!(settings.validate().is_err());
    }
    #[test]
    fn screening_must_still_have_inclusion_margin_after_approval() {
        let r = json!({"screening_attached":true,"screening_issued_at":1000});
        assert!(screening_fresh(&request("deposit"), &r, 1000).is_ok());
        assert!(screening_fresh(&request("deposit"), &r, 1239).is_ok());
        assert!(screening_fresh(&request("deposit"), &r, 1240).is_err());
        assert!(screening_fresh(&request("deposit"), &r, 969).is_err());
        assert!(screening_fresh(
            &request("deposit"),
            &json!({"screening_attached":true}),
            1000
        )
        .is_err());
        assert!(screening_fresh(&request("transfer"), &r, 1240).is_ok());
    }
    #[tokio::test]
    async fn screening_observation_is_persistent_and_configuration_bound() {
        let mut random = [0u8; 16];
        getrandom::getrandom(&mut random).unwrap();
        let path = std::env::temp_dir().join(format!("gravity-screening-{}", hex::encode(random)));
        let state = PrivacyState::new(path.clone(), PathBuf::new(), PathBuf::new());
        let config = json!({"screening_policy":"required","pool_address":"0x123","prover_url":"http://127.0.0.1:3000","chain_id":"0x534e5f4d41494e"});
        assert_eq!(state.deposit_screening(&config), "unverified");
        state.observe_screening(&config, true).unwrap();
        let restarted = PrivacyState::new(path.clone(), PathBuf::new(), PathBuf::new());
        assert_eq!(restarted.deposit_screening(&config), "signature_missing");
        for field in ["pool_address", "prover_url", "chain_id", "deposit_prover"] {
            let mut changed = config.clone();
            changed[field] = json!("changed");
            assert_eq!(restarted.deposit_screening(&changed), "unverified");
        }
        let mut custom = config.clone();
        custom["screening_policy"] = json!("pool_enforced");
        assert_eq!(restarted.deposit_screening(&custom), "pool_enforced");
        restarted.observe_screening(&config, false).unwrap();
        assert_eq!(restarted.deposit_screening(&config), "unverified");
        restarted.observe_screening(&config, true).unwrap();
        restarted.set_settings(Settings::default()).await.unwrap();
        assert_eq!(restarted.deposit_screening(&config), "unverified");
        let saved = std::fs::read_to_string(path.join("screening-observation.json")).unwrap();
        assert!(!saved.contains("prover_url"));
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn decimal_callback_fields_are_not_parsed_as_hex() {
        assert_eq!(felt("10").unwrap(), Felt::from(10u64));
        assert_eq!(
            canonical(&json!({"nonce":"10","type":"INVOKE"})),
            canonical(&json!({"nonce":"0xa","type":"INVOKE"}))
        );
    }
    #[test]
    fn built_in_pool_cannot_disable_required_screening() {
        let mut settings = Settings::default();
        settings.mainnet.screening_policy = ScreeningPolicy::PoolEnforced;
        assert!(settings.validate().is_err());
        settings.mainnet.pool_address = "0x123".into();
        assert!(settings.validate().is_ok());
        settings.mainnet.pool_address = "0x0".into();
        assert!(settings.validate().is_err());
    }
}
