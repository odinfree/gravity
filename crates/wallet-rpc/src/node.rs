//! Starknet node client — the I/O boundary for fee estimation, nonce lookup,
//! and broadcasting (spec §7.4, Phase 2).
//!
//! `StarknetRpc` is an injectable seam (like `Approver`): the dispatch
//! orchestration (fetch nonce → estimate → sign → broadcast) is written against
//! the trait and is fully testable with a mock. `HttpStarknetRpc` is the real
//! reqwest JSON-RPC implementation.
//!
//! Wire format: **verified against a live Sepolia v0.10 node** (2026-06-09) for
//! `getNonce`, `getClassHashAt` (deploy status), and `estimateFee`
//! request+response (the 3-resource fee model, V3 tx JSON, DA modes), including
//! the http→https redirect. The block tag is `latest` (v0.10 dropped `pending`).
//! The only path not yet confirmed end-to-end is the actual **broadcast**
//! (`add_invoke` / `add_deploy_account`), which needs a real signed tx from a
//! funded account; its tx object is the same one `estimateFee` already accepted,
//! so the remaining risk is just the submit wrapper. The trait keeps any future
//! fix localized (or swap in `starknet-rs`).

use async_trait::async_trait;
use serde_json::{json, Value};
use wallet_core::{Felt, ResourceBounds};

/// The three V3 resource bounds.
#[derive(Debug, Clone, Copy)]
pub struct FeeBounds {
    pub l1_gas: ResourceBounds,
    pub l2_gas: ResourceBounds,
    pub l1_data_gas: ResourceBounds,
}

#[derive(Debug, Clone)]
pub enum NodeError {
    Transport(String),
    Rpc(String),
    Decode(String),
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeError::Transport(m) => write!(f, "transport: {m}"),
            NodeError::Rpc(m) => write!(f, "rpc: {m}"),
            NodeError::Decode(m) => write!(f, "decode: {m}"),
        }
    }
}

/// Minimal Starknet JSON-RPC surface the wallet needs.
#[async_trait]
pub trait StarknetRpc: Send + Sync {
    /// Receipt + transaction + canonical block proof for the original treasury invoke.
    /// Unsupported implementations fail closed; no mutation or signature replay.
    async fn accepted_invoke(&self, _hash: &Felt, _sender: &Felt, _nonce: &Felt) -> Result<bool, NodeError> {
        Err(NodeError::Decode("accepted invoke evidence unavailable".into()))
    }
    /// Explicit node network check required by the narrow treasury path.
    async fn chain_id(&self) -> Result<Felt, NodeError> { Err(NodeError::Decode("chain identity unavailable".into())) }
    /// Current nonce of `address` (pending block).
    async fn get_nonce(&self, address: &Felt) -> Result<Felt, NodeError>;

    /// Estimate fee for a V3 invoke and return resource bounds (safety margin
    /// already applied). Estimation uses SKIP_VALIDATE, so no signature needed.
    async fn estimate_invoke(
        &self,
        sender: &Felt,
        calldata: &[Felt],
        nonce: &Felt,
    ) -> Result<FeeBounds, NodeError>;

    /// Broadcast a signed V3 invoke; returns the transaction hash. For a SNIP-36
    /// proof-carrying invoke, `proof_facts` is non-empty and `proof` carries the
    /// base64 STWO proof (both ride along on broadcast).
    #[allow(clippy::too_many_arguments)]
    async fn add_invoke(
        &self,
        sender: &Felt,
        calldata: &[Felt],
        signature: &[Felt],
        nonce: &Felt,
        bounds: &FeeBounds,
        proof_facts: &[Felt],
        proof: Option<&str>,
    ) -> Result<Felt, NodeError>;

    /// Whether `address` has a deployed contract class (false = counterfactual).
    async fn is_deployed(&self, address: &Felt) -> Result<bool, NodeError>;

    /// ERC-20 `balanceOf(holder)` on `token`, as the low 128 bits (fri for STRK).
    /// Realistic balances fit in u128; a non-zero high word is treated as
    /// saturating-max so the UI never under-reports.
    async fn balance_of(&self, token: &Felt, holder: &Felt) -> Result<u128, NodeError>;

    /// Estimate the fee to deploy an OZ account (nonce 0, SKIP_VALIDATE).
    async fn estimate_deploy_account(
        &self,
        address: &Felt,
        class_hash: &Felt,
        constructor_calldata: &[Felt],
        salt: &Felt,
    ) -> Result<FeeBounds, NodeError>;

    /// Broadcast a signed DEPLOY_ACCOUNT v3; returns the transaction hash.
    #[allow(clippy::too_many_arguments)]
    async fn add_deploy_account(
        &self,
        class_hash: &Felt,
        constructor_calldata: &[Felt],
        salt: &Felt,
        signature: &[Felt],
        bounds: &FeeBounds,
    ) -> Result<Felt, NodeError>;

    /// Estimate the fee to declare a class (needs the full Sierra contract class).
    async fn estimate_declare(
        &self,
        sender: &Felt,
        compiled_class_hash: &Felt,
        contract_class: &Value,
        nonce: &Felt,
    ) -> Result<FeeBounds, NodeError>;

    /// Broadcast a signed DECLARE v3; returns the transaction hash.
    #[allow(clippy::too_many_arguments)]
    async fn add_declare(
        &self,
        sender: &Felt,
        compiled_class_hash: &Felt,
        contract_class: &Value,
        signature: &[Felt],
        nonce: &Felt,
        bounds: &FeeBounds,
    ) -> Result<Felt, NodeError>;
}

fn fh(f: &Felt) -> String {
    format!("0x{:x}", f)
}

fn u128_to_hex(v: u128) -> String {
    format!("0x{v:x}")
}

/// A complete, canonical-hex RPC `INVOKE_TXN_V3` object. Shared by broadcast and
/// the sign-only response so callers get a **ready-to-broadcast** transaction
/// (all fields present, hex-normalized) rather than a partial one. With
/// `signature: &[]` and zero bounds it's also the estimate-request body.
/// `proof_facts`/`proof` are added when non-empty (SNIP-36).
pub fn invoke_v3_tx_json(
    sender: &Felt,
    calldata: &[Felt],
    signature: &[Felt],
    nonce: &Felt,
    bounds: &FeeBounds,
    proof_facts: &[Felt],
    proof: Option<&str>,
) -> Value {
    let rb = |b: &ResourceBounds| {
        json!({
            "max_amount": u128_to_hex(b.max_amount as u128),
            "max_price_per_unit": u128_to_hex(b.max_price_per_unit),
        })
    };
    let mut tx = json!({
        "type": "INVOKE",
        "version": "0x3",
        // Zero-padded to 64 hex, matching strkd's account addresses elsewhere so
        // callers can compare sender_address directly.
        "sender_address": format!("0x{:064x}", sender),
        "calldata": calldata.iter().map(fh).collect::<Vec<_>>(),
        "signature": signature.iter().map(fh).collect::<Vec<_>>(),
        "nonce": fh(nonce),
        "resource_bounds": {
            "l1_gas": rb(&bounds.l1_gas),
            "l2_gas": rb(&bounds.l2_gas),
            "l1_data_gas": rb(&bounds.l1_data_gas),
        },
        "tip": "0x0",
        "paymaster_data": [],
        "account_deployment_data": [],
        "nonce_data_availability_mode": "L1",
        "fee_data_availability_mode": "L1",
    });
    if !proof_facts.is_empty() {
        let obj = tx.as_object_mut().expect("object");
        obj.insert(
            "proof_facts".into(),
            json!(proof_facts.iter().map(fh).collect::<Vec<_>>()),
        );
        if let Some(p) = proof {
            obj.insert("proof".into(), json!(p));
        }
    }
    tx
}

/// Block tag for "most recent" queries. v0.10 dropped the legacy `pending` tag;
/// `latest` is valid across spec versions (verified against a v0.10 node).
const BLOCK_TAG: &str = "latest";

/// Real JSON-RPC client over reqwest. See the module-level wire-format caveat.
pub struct HttpStarknetRpc {
    /// Effective endpoint. Some providers redirect http→https; we re-POST to the
    /// redirect target (reqwest would turn a 301 POST into a GET) and cache it.
    url: std::sync::RwLock<String>,
    client: reqwest::Client,
    /// Multiply estimated amounts/prices by this (×100, integer) for headroom.
    margin_pct: u128,
}

impl HttpStarknetRpc {
    pub fn new(url: impl Into<String>) -> Self {
        // Don't auto-follow redirects: reqwest downgrades a 301 POST to GET and
        // drops the body. We follow once manually, preserving the POST.
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        HttpStarknetRpc {
            url: std::sync::RwLock::new(url.into()),
            client,
            margin_pct: 150, // 1.5×
        }
    }

    /// Strict bounded read-only transport for treasury recovery; no redirect or retry.
    async fn treasury_read(&self, method: &str, params: Value) -> Result<Value, NodeError> {
        let target = self.url.read().unwrap().clone();
        let mut response = self
            .client
            .post(&target)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
            .await
            .map_err(|_| NodeError::Transport("treasury evidence unavailable".into()))?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(NodeError::Decode("treasury evidence HTTP status".into()));
        }
        const MAX: usize = 2 * 1024 * 1024;
        if response.content_length().is_some_and(|n| n > MAX as u64) {
            return Err(NodeError::Decode("treasury evidence size limit".into()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| NodeError::Decode("treasury evidence body".into()))?
        {
            if bytes.len() + chunk.len() > MAX {
                return Err(NodeError::Decode("treasury evidence size limit".into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        let v: Value = serde_json::from_slice(&bytes)
            .map_err(|_| NodeError::Decode("treasury evidence JSON".into()))?;
        if v["jsonrpc"] != "2.0"
            || v["id"] != 1
            || v.get("error").is_some()
            || v.as_object().is_none_or(|o| o.len() != 3)
        {
            return Err(NodeError::Decode("treasury evidence envelope".into()));
        }
        v.get("result")
            .cloned()
            .ok_or_else(|| NodeError::Decode("treasury evidence result".into()))
    }
    async fn call(&self, method: &str, params: Value) -> Result<Value, NodeError> {
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        let target = self.url.read().unwrap().clone();
        let mut resp = self
            .client
            .post(&target)
            .json(&body)
            .send()
            .await
            .map_err(|e| NodeError::Transport(e.to_string()))?;

        // One-hop redirect (e.g. http→https), re-POSTing the body, then cache it.
        if resp.status().is_redirection() {
            if let Some(loc) = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
            {
                resp = self
                    .client
                    .post(&loc)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| NodeError::Transport(e.to_string()))?;
                *self.url.write().unwrap() = loc;
            }
        }

        let v: Value = resp
            .json()
            .await
            .map_err(|e| NodeError::Decode(e.to_string()))?;
        if let Some(err) = v.get("error") {
            return Err(NodeError::Rpc(err.to_string()));
        }
        v.get("result")
            .cloned()
            .ok_or_else(|| NodeError::Decode("missing result".into()))
    }

    fn parse_felt(v: &Value, what: &str) -> Result<Felt, NodeError> {
        let s = v
            .as_str()
            .ok_or_else(|| NodeError::Decode(format!("{what}: expected hex string")))?;
        Felt::from_hex(s).map_err(|_| NodeError::Decode(format!("{what}: bad felt {s}")))
    }

    fn parse_u128(v: &Value, what: &str) -> Result<u128, NodeError> {
        let s = v
            .as_str()
            .ok_or_else(|| NodeError::Decode(format!("{what}: expected hex string")))?;
        let s = s.strip_prefix("0x").unwrap_or(s);
        u128::from_str_radix(s, 16).map_err(|_| NodeError::Decode(format!("{what}: bad u128")))
    }

    fn with_margin(&self, v: u128) -> u128 {
        v.saturating_mul(self.margin_pct) / 100
    }


    /// DEPLOY_ACCOUNT v3 tx JSON shared by estimate (empty sig, zero bounds) and
    /// broadcast (real sig + bounds). Nonce is always 0.
    fn deploy_account_tx_json(
        class_hash: &Felt,
        constructor_calldata: &[Felt],
        salt: &Felt,
        signature: &[Felt],
        bounds: &FeeBounds,
    ) -> Value {
        let rb = |b: &ResourceBounds| {
            json!({
                "max_amount": u128_to_hex(b.max_amount as u128),
                "max_price_per_unit": u128_to_hex(b.max_price_per_unit),
            })
        };
        json!({
            "type": "DEPLOY_ACCOUNT",
            "version": "0x3",
            "class_hash": fh(class_hash),
            "contract_address_salt": fh(salt),
            "constructor_calldata": constructor_calldata.iter().map(fh).collect::<Vec<_>>(),
            "signature": signature.iter().map(fh).collect::<Vec<_>>(),
            "nonce": "0x0",
            "resource_bounds": {
                "l1_gas": rb(&bounds.l1_gas),
                "l2_gas": rb(&bounds.l2_gas),
                "l1_data_gas": rb(&bounds.l1_data_gas),
            },
            "tip": "0x0",
            "paymaster_data": [],
            "nonce_data_availability_mode": "L1",
            "fee_data_availability_mode": "L1",
        })
    }

    fn bounds_from_estimate(&self, est: &Value) -> Result<FeeBounds, NodeError> {
        let bound = |amount_key: &str, price_key: &str| -> Result<ResourceBounds, NodeError> {
            Ok(ResourceBounds {
                max_amount: self.with_margin(Self::parse_u128(&est[amount_key], amount_key)?) as u64,
                max_price_per_unit: self.with_margin(Self::parse_u128(&est[price_key], price_key)?),
            })
        };
        Ok(FeeBounds {
            l1_gas: bound("l1_gas_consumed", "l1_gas_price")?,
            l2_gas: bound("l2_gas_consumed", "l2_gas_price")?,
            l1_data_gas: bound("l1_data_gas_consumed", "l1_data_gas_price")?,
        })
    }
}

const ZERO_BOUNDS: FeeBounds = FeeBounds {
    l1_gas: ResourceBounds {
        max_amount: 0,
        max_price_per_unit: 0,
    },
    l2_gas: ResourceBounds {
        max_amount: 0,
        max_price_per_unit: 0,
    },
    l1_data_gas: ResourceBounds {
        max_amount: 0,
        max_price_per_unit: 0,
    },
};

#[async_trait]
impl StarknetRpc for HttpStarknetRpc {
    async fn accepted_invoke(
        &self,
        hash: &Felt,
        sender: &Felt,
        nonce: &Felt,
    ) -> Result<bool, NodeError> {
        let receipt = self
            .treasury_read(
                "starknet_getTransactionReceipt",
                json!({"transaction_hash":fh(hash)}),
            )
            .await?;
        let Some(number) = receipt.get("block_number").and_then(Value::as_u64) else {
            return Ok(false);
        };
        let transaction = self
            .treasury_read(
                "starknet_getTransactionByHash",
                json!({"transaction_hash":fh(hash)}),
            )
            .await?;
        let block = self
            .treasury_read(
                "starknet_getBlockWithTxHashes",
                json!({"block_id":{"block_number":number}}),
            )
            .await?;
        Ok(accepted_invoke_evidence(
            hash,
            sender,
            nonce,
            &receipt,
            &transaction,
            &block,
        ))
    }
    async fn chain_id(&self) -> Result<Felt, NodeError> {
        let r = self.call("starknet_chainId", json!([])).await?;
        Self::parse_felt(&r, "chain_id")
    }
    async fn get_nonce(&self, address: &Felt) -> Result<Felt, NodeError> {
        let r = self
            .call("starknet_getNonce", json!([BLOCK_TAG, fh(address)]))
            .await?;
        Self::parse_felt(&r, "nonce")
    }

    async fn estimate_invoke(
        &self,
        sender: &Felt,
        calldata: &[Felt],
        nonce: &Felt,
    ) -> Result<FeeBounds, NodeError> {
        let tx = invoke_v3_tx_json(sender, calldata, &[], nonce, &ZERO_BOUNDS, &[], None);
        let r = self
            .call(
                "starknet_estimateFee",
                json!({ "request": [tx], "simulation_flags": ["SKIP_VALIDATE"], "block_id": BLOCK_TAG }),
            )
            .await?;
        let est = r
            .get(0)
            .ok_or_else(|| NodeError::Decode("empty estimate array".into()))?;
        // Spec 0.8 FEE_ESTIMATE fields. Amounts/prices get a safety margin.
        self.bounds_from_estimate(est)
    }

    async fn add_invoke(
        &self,
        sender: &Felt,
        calldata: &[Felt],
        signature: &[Felt],
        nonce: &Felt,
        bounds: &FeeBounds,
        proof_facts: &[Felt],
        proof: Option<&str>,
    ) -> Result<Felt, NodeError> {
        // SNIP-36 proof fields (when present) are attached by invoke_v3_tx_json.
        // NOTE: the exact SNIP-36 field names on a given node are not verified
        // here — see the module-level wire-format note.
        let tx = invoke_v3_tx_json(sender, calldata, signature, nonce, bounds, proof_facts, proof);
        let r = self
            .call("starknet_addInvokeTransaction", json!({ "invoke_transaction": tx }))
            .await?;
        Self::parse_felt(&r["transaction_hash"], "transaction_hash")
    }

    async fn is_deployed(&self, address: &Felt) -> Result<bool, NodeError> {
        // getClassHashAt succeeds for a deployed account; "contract not found"
        // (a normal Rpc error here) means counterfactual/undeployed.
        match self
            .call("starknet_getClassHashAt", json!([BLOCK_TAG, fh(address)]))
            .await
        {
            Ok(_) => Ok(true),
            Err(NodeError::Rpc(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    async fn balance_of(&self, token: &Felt, holder: &Felt) -> Result<u128, NodeError> {
        // balanceOf returns a u256 as [low, high].
        let r = self
            .call(
                "starknet_call",
                json!({
                    "request": {
                        "contract_address": fh(token),
                        "entry_point_selector": fh(&wallet_core::get_selector_from_name("balanceOf")),
                        "calldata": [fh(holder)],
                    },
                    "block_id": BLOCK_TAG,
                }),
            )
            .await?;
        let low = r
            .get(0)
            .ok_or_else(|| NodeError::Decode("balanceOf: empty result".into()))?;
        let high_nonzero = r
            .get(1)
            .and_then(|v| v.as_str())
            .map(|s| Felt::from_hex(s).map(|f| f != Felt::ZERO).unwrap_or(false))
            .unwrap_or(false);
        if high_nonzero {
            return Ok(u128::MAX); // absurdly large; saturate rather than under-report
        }
        Self::parse_u128(low, "balance")
    }

    async fn estimate_deploy_account(
        &self,
        _address: &Felt,
        class_hash: &Felt,
        constructor_calldata: &[Felt],
        salt: &Felt,
    ) -> Result<FeeBounds, NodeError> {
        let tx = Self::deploy_account_tx_json(class_hash, constructor_calldata, salt, &[], &ZERO_BOUNDS);
        let r = self
            .call(
                "starknet_estimateFee",
                json!({ "request": [tx], "simulation_flags": ["SKIP_VALIDATE"], "block_id": BLOCK_TAG }),
            )
            .await?;
        let est = r
            .get(0)
            .ok_or_else(|| NodeError::Decode("empty estimate array".into()))?;
        self.bounds_from_estimate(est)
    }

    async fn add_deploy_account(
        &self,
        class_hash: &Felt,
        constructor_calldata: &[Felt],
        salt: &Felt,
        signature: &[Felt],
        bounds: &FeeBounds,
    ) -> Result<Felt, NodeError> {
        let tx = Self::deploy_account_tx_json(class_hash, constructor_calldata, salt, signature, bounds);
        let r = self
            .call(
                "starknet_addDeployAccountTransaction",
                json!({ "deploy_account_transaction": tx }),
            )
            .await?;
        Self::parse_felt(&r["transaction_hash"], "transaction_hash")
    }

    async fn estimate_declare(
        &self,
        sender: &Felt,
        compiled_class_hash: &Felt,
        contract_class: &Value,
        nonce: &Felt,
    ) -> Result<FeeBounds, NodeError> {
        let tx =
            declare_v3_tx_json(sender, compiled_class_hash, Some(contract_class), &[], nonce, &ZERO_BOUNDS);
        let r = self
            .call(
                "starknet_estimateFee",
                json!({ "request": [tx], "simulation_flags": ["SKIP_VALIDATE"], "block_id": BLOCK_TAG }),
            )
            .await?;
        let est = r
            .get(0)
            .ok_or_else(|| NodeError::Decode("empty estimate array".into()))?;
        self.bounds_from_estimate(est)
    }

    async fn add_declare(
        &self,
        sender: &Felt,
        compiled_class_hash: &Felt,
        contract_class: &Value,
        signature: &[Felt],
        nonce: &Felt,
        bounds: &FeeBounds,
    ) -> Result<Felt, NodeError> {
        let tx = declare_v3_tx_json(
            sender,
            compiled_class_hash,
            Some(contract_class),
            signature,
            nonce,
            bounds,
        );
        let r = self
            .call("starknet_addDeclareTransaction", json!({ "declare_transaction": tx }))
            .await?;
        Self::parse_felt(&r["transaction_hash"], "transaction_hash")
    }
}

/// A complete, canonical-hex RPC `BROADCASTED_DECLARE_TXN_V3` object. Shared by
/// estimate, broadcast and the sign-only response so callers get a
/// **ready-to-broadcast** transaction. `contract_class` is the canonical RPC
/// `CONTRACT_CLASS` (see `wallet_core::SierraClass::to_rpc_json`) — the node
/// derives the class hash from it — and is omitted from the object when `None`
/// (sign-only without the class; the caller splices theirs in).
pub fn declare_v3_tx_json(
    sender: &Felt,
    compiled_class_hash: &Felt,
    contract_class: Option<&Value>,
    signature: &[Felt],
    nonce: &Felt,
    bounds: &FeeBounds,
) -> Value {
    let rb = |b: &ResourceBounds| {
        json!({
            "max_amount": u128_to_hex(b.max_amount as u128),
            "max_price_per_unit": u128_to_hex(b.max_price_per_unit),
        })
    };
    let mut tx = json!({
        "type": "DECLARE",
        "version": "0x3",
        "sender_address": fh(sender),
        "compiled_class_hash": fh(compiled_class_hash),
        "signature": signature.iter().map(fh).collect::<Vec<_>>(),
        "nonce": fh(nonce),
        "resource_bounds": {
            "l1_gas": rb(&bounds.l1_gas),
            "l2_gas": rb(&bounds.l2_gas),
            "l1_data_gas": rb(&bounds.l1_data_gas),
        },
        "tip": "0x0",
        "paymaster_data": [],
        "account_deployment_data": [],
        "nonce_data_availability_mode": "L1",
        "fee_data_availability_mode": "L1",
    });
    if let Some(cc) = contract_class {
        tx["contract_class"] = cc.clone();
    }
    tx
}

/// Configured-provider evidence, not a light-client proof. Reverted acceptance
/// also consumes the nonce; it never establishes principal transfer success.
fn accepted_invoke_evidence(
    hash: &Felt,
    sender: &Felt,
    nonce: &Felt,
    r: &Value,
    t: &Value,
    b: &Value,
) -> bool {
    let f = |v: &Value| v.as_str().and_then(|s| Felt::from_hex(s).ok());
    let accepted = |v: &Value| matches!(v.as_str(), Some("ACCEPTED_ON_L1" | "ACCEPTED_ON_L2"));
    let Some(block_hash) = f(&r["block_hash"]) else {
        return false;
    };
    if block_hash == Felt::ZERO
        || !accepted(&r["finality_status"])
        || !matches!(
            r["execution_status"].as_str(),
            Some("SUCCEEDED" | "REVERTED")
        )
        || r["type"] != "INVOKE"
        || f(&r["transaction_hash"]) != Some(*hash)
        || t["type"] != "INVOKE"
        || f(&t["version"]) != Some(Felt::from(3u64))
        || f(&t["transaction_hash"]) != Some(*hash)
        || f(&t["sender_address"]) != Some(*sender)
        || f(&t["nonce"]) != Some(*nonce)
        || !accepted(&b["status"])
        || f(&b["block_hash"]) != Some(block_hash)
        || r["block_number"].as_u64().is_none()
        || r["block_number"].as_u64() != b["block_number"].as_u64()
    {
        return false;
    }
    let Some(txs) = b["transactions"].as_array() else {
        return false;
    };
    if txs.len() > 20000 {
        return false;
    }
    let mut unique = std::collections::HashSet::new();
    for tx in txs {
        let Some(tx) = f(tx) else {
            return false;
        };
        if tx == Felt::ZERO || !unique.insert(tx) {
            return false;
        }
    }
    unique.contains(hash)
}

#[cfg(test)]
mod treasury_recovery_tests {
    use super::*;
    fn evidence() -> (Value, Value, Value) {
        (
            json!({"type":"INVOKE","transaction_hash":"0x11","finality_status":"ACCEPTED_ON_L2","execution_status":"SUCCEEDED","block_hash":"0x44","block_number":7}),
            json!({"type":"INVOKE","version":"0x3","transaction_hash":"0x11","sender_address":"0x22","nonce":"0x3"}),
            json!({"status":"ACCEPTED_ON_L2","block_hash":"0x44","block_number":7,"transactions":["0x11","0x12"]}),
        )
    }
    fn valid(r: &Value, t: &Value, b: &Value) -> bool {
        accepted_invoke_evidence(
            &Felt::from(17u64),
            &Felt::from(34u64),
            &Felt::from(3u64),
            r,
            t,
            b,
        )
    }
    #[test]
    fn accepted_evidence_requires_all_identity_and_canonical_inclusion_fields() {
        let (r, t, b) = evidence();
        assert!(valid(&r, &t, &b));
        let mut reverted = r.clone();
        reverted["execution_status"] = json!("REVERTED");
        assert!(valid(&reverted, &t, &b));
        for (key, value) in [
            ("block_hash", json!("0x45")),
            ("block_number", json!(8)),
            ("transaction_hash", json!("0x12")),
            ("finality_status", json!("PRE_CONFIRMED")),
            ("execution_status", json!("UNKNOWN")),
            ("type", json!("DECLARE")),
        ] {
            let mut x = r.clone();
            x[key] = value;
            assert!(!valid(&x, &t, &b), "receipt {key}");
        }
        for key in ["block_hash", "block_number", "execution_status"] {
            let mut x = r.clone();
            x.as_object_mut().unwrap().remove(key);
            assert!(!valid(&x, &t, &b));
        }
        for (key, value) in [
            ("transaction_hash", json!("0x12")),
            ("sender_address", json!("0x23")),
            ("nonce", json!("0x4")),
            ("version", json!("0x1")),
            ("type", json!("DECLARE")),
        ] {
            let mut x = t.clone();
            x[key] = value;
            assert!(!valid(&r, &x, &b), "transaction {key}");
        }
        for (key, value) in [
            ("block_hash", json!("0x45")),
            ("block_number", json!(8)),
            ("status", json!("PRE_CONFIRMED")),
            ("transactions", json!(["0x12"])),
            ("transactions", json!(["0x11", "0x011"])),
            ("transactions", json!(["0x11", "0x0"])),
            ("transactions", json!(["0x11", "garbage"])),
        ] {
            let mut x = b.clone();
            x[key] = value;
            assert!(!valid(&r, &t, &x), "block {key}");
        }
    }
    #[tokio::test]
    async fn recovery_http_uses_only_exact_read_methods_and_rejects_bad_envelopes() {
        use axum::{routing::post, Json, Router};
        use std::sync::{Arc, Mutex};
        let calls = Arc::new(Mutex::new(Vec::<Value>::new()));
        let seen = calls.clone();
        let app = Router::new().route(
            "/",
            post(move |Json(p): Json<Value>| {
                let seen = seen.clone();
                async move {
                    seen.lock().unwrap().push(p.clone());
                    let (r, t, b) = evidence();
                    let result = match p["method"].as_str().unwrap() {
                        "starknet_getTransactionReceipt" => {
                            assert_eq!(p["params"], json!({"transaction_hash":"0x11"}));
                            r
                        }
                        "starknet_getTransactionByHash" => {
                            assert_eq!(p["params"], json!({"transaction_hash":"0x11"}));
                            t
                        }
                        "starknet_getBlockWithTxHashes" => {
                            assert_eq!(p["params"], json!({"block_id":{"block_number":7}}));
                            b
                        }
                        _ => panic!("unexpected method"),
                    };
                    Json(json!({"jsonrpc":"2.0","id":1,"result":result}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let node = HttpStarknetRpc::new(format!("http://{addr}"));
        assert!(node
            .accepted_invoke(&Felt::from(17u64), &Felt::from(34u64), &Felt::from(3u64))
            .await
            .unwrap());
        assert_eq!(calls.lock().unwrap().len(), 3);
        task.abort();
        for response in [
            json!({"jsonrpc":"2.0","id":2,"result":{}}),
            json!({"jsonrpc":"2.0","id":1,"result":{},"error":{}}),
            json!({"jsonrpc":"2.0","id":1,"error":{"message":"credential-canary"}}),
        ] {
            let app = Router::new().route(
                "/",
                post(move || {
                    let r = response.clone();
                    async move { Json(r) }
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let node = HttpStarknetRpc::new(format!("http://{addr}"));
            let err = node
                .accepted_invoke(&Felt::from(17u64), &Felt::from(34u64), &Felt::from(3u64))
                .await
                .unwrap_err();
            assert!(!err.to_string().contains("credential-canary"));
            task.abort();
        }
    }
}
