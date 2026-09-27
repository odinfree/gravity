//! Adapter for a self-hosted Starknet transaction-prover JSON-RPC service.
//! Sends the supplied virtual transaction unchanged; never signs or broadcasts.
use std::{net::IpAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use reqwest::{Client, Url};
use serde_json::{json, Value};

use crate::{ProveRequest, ProveResult, Prover, SettingsStore};

const MAX_RESPONSE: usize = 32 * 1024 * 1024;

pub struct StarknetRpcProver {
    settings: Arc<SettingsStore>,
    http: Client,
}

impl StarknetRpcProver {
    pub fn new(settings: Arc<SettingsStore>) -> Self {
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(900))
            .build()
            .expect("construct Starknet prover HTTP client");
        Self { settings, http }
    }

    /// Read-only protocol check; it does not establish proof correctness or chain identity.
    pub async fn check_connection(&self, network: &str) -> Result<String, String> {
        expected_chain(network)?;
        let net = self.settings.for_network(network).await;
        self.check_endpoint(&net.prover_url).await
    }

    async fn check_endpoint(&self, endpoint: &str) -> Result<String, String> {
        let version = self
            .rpc(endpoint, "starknet_specVersion", json!([]), 15)
            .await?;
        let version = version.as_str().ok_or("invalid prover spec version")?;
        if version != "0.10" && !version.starts_with("0.10.") {
            return Err("self-hosted prover must expose Starknet RPC v0.10".into());
        }
        Ok(version.to_string())
    }

    async fn rpc(
        &self,
        endpoint: &str,
        method: &str,
        params: Value,
        timeout: u64,
    ) -> Result<Value, String> {
        let url = endpoint_url(endpoint)?;
        let mut response = self
            .http
            .post(url)
            .json(&json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}))
            .timeout(Duration::from_secs(timeout))
            .send()
            .await
            .map_err(|_| {
                format!("{method}: connection failed or timed out; endpoint details withheld")
            })?;
        if !response.status().is_success() {
            return Err(format!(
                "{method}: HTTP {}; response body withheld",
                response.status().as_u16()
            ));
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_RESPONSE as u64)
        {
            return Err("prover response exceeds 32 MiB".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "failed to read prover response")?
        {
            if bytes.len() + chunk.len() > MAX_RESPONSE {
                return Err("prover response exceeds 32 MiB".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let body: Value =
            serde_json::from_slice(&bytes).map_err(|_| "invalid prover JSON response")?;
        if body.get("jsonrpc") != Some(&json!("2.0")) || body.get("id") != Some(&json!(1)) {
            return Err("unexpected JSON-RPC response envelope".into());
        }
        if let Some(error) = body.get("error") {
            return Err(match error.get("code").and_then(Value::as_i64) {
                Some(-32005) => "prover busy; wait for the active proof before retrying".into(),
                Some(code) => format!("{method}: RPC error {code}; raw error withheld"),
                None => format!("{method}: RPC error; raw error withheld"),
            });
        }
        body.get("result")
            .cloned()
            .ok_or_else(|| "JSON-RPC response has no result".into())
    }
}

fn expected_chain(network: &str) -> Result<&'static str, String> {
    match network {
        "mainnet" => Ok("0x534e5f4d41494e"),
        "testnet" | "sepolia" => Ok("0x534e5f5345504f4c4941"),
        _ => Err("select mainnet or testnet explicitly".into()),
    }
}

fn endpoint_url(endpoint: &str) -> Result<Url, String> {
    if endpoint.is_empty()
        || endpoint
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err("configure a valid endpoint in Settings".into());
    }
    let url = Url::parse(endpoint).map_err(|_| "invalid endpoint URL; details withheld")?;
    let host = url.host_str().unwrap_or("");
    let loopback = host == "localhost"
        || host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
    {
        return Err("use HTTPS or loopback HTTP without URL userinfo/fragments; tunnel a remote host over SSH".into());
    }
    Ok(url)
}

fn zero_hex(value: &Value) -> bool {
    value
        .as_str()
        .and_then(|s| s.strip_prefix("0x"))
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b == b'0'))
}

fn validate_transaction(tx: &Value) -> Result<(), String> {
    if tx.get("type") != Some(&json!("INVOKE")) || tx.get("version") != Some(&json!("0x3")) {
        return Err("supply a signed INVOKE v3 virtual transaction".into());
    }
    if !tx
        .get("signature")
        .and_then(Value::as_array)
        .is_some_and(|s| !s.is_empty() && s.iter().all(Value::is_string))
    {
        return Err("virtual transaction needs a signature; this backend never signs".into());
    }
    // Do not rewrite fee fields after signing: that would invalidate the signature.
    if !tx.get("tip").is_some_and(zero_hex) {
        return Err("sign the virtual transaction with zero tip".into());
    }
    for resource in ["l1_gas", "l2_gas", "l1_data_gas"] {
        let bound = &tx["resource_bounds"][resource];
        if !bound.get("max_amount").is_some_and(zero_hex)
            && !bound.get("max_price_per_unit").is_some_and(zero_hex)
        {
            return Err(
                "sign the virtual transaction with zero effective fee for every resource".into(),
            );
        }
    }
    Ok(())
}

fn validate_result(result: &Value) -> Result<(), String> {
    let strings = |v: &Value| v.as_array().is_some_and(|a| a.iter().all(Value::is_string));
    if !result
        .get("proof")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty())
        || !strings(&result["proof_facts"])
        || !result
            .get("l2_to_l1_messages")
            .and_then(Value::as_array)
            .is_some_and(|messages| {
                messages.iter().all(|m| {
                    m["from_address"].is_string()
                        && m["to_address"].is_string()
                        && strings(&m["payload"])
                })
            })
    {
        return Err(
            "invalid proof result; expected proof, proof_facts and l2_to_l1_messages".into(),
        );
    }
    Ok(())
}

#[async_trait]
impl Prover for StarknetRpcProver {
    async fn prove(&self, req: ProveRequest) -> Result<ProveResult, String> {
        let chain = expected_chain(&req.network)?;
        let tx = req
            .payload
            .get("transaction")
            .ok_or("payload missing transaction")?;
        validate_transaction(tx)?;
        let requested_block = match req.payload.get("block_number") {
            Some(value) => Some(
                value
                    .as_u64()
                    .ok_or("block_number must be an unsigned integer")?,
            ),
            None => None,
        };
        let net = self.settings.for_network(&req.network).await;
        // Only public chain metadata goes to the node; private calldata goes to the prover.
        let actual_chain = self
            .rpc(&net.rpc_url, "starknet_chainId", json!([]), 15)
            .await?;
        if actual_chain.as_str() != Some(chain) {
            return Err("RPC node network does not match the proving request".into());
        }
        self.check_endpoint(&net.prover_url).await?;
        let block_number = match requested_block {
            Some(n) => n,
            None => self
                .rpc(&net.rpc_url, "starknet_blockNumber", json!([]), 15)
                .await?
                .as_u64()
                .and_then(|n| n.checked_sub(10))
                .ok_or("cannot select a finalized proof base")?,
        };
        let proof = self
            .rpc(
                &net.prover_url,
                "starknet_proveTransaction",
                json!({"block_id": {"block_number": block_number}, "transaction": tx}),
                900,
            )
            .await?;
        validate_result(&proof)?;
        Ok(ProveResult { proof })
    }

    fn kind(&self) -> &'static str {
        "starknet-rpc"
    }

    fn ready(&self) -> bool {
        self.settings.try_snapshot().is_some_and(|s| {
            [&s.mainnet, &s.testnet].iter().any(|net| {
                endpoint_url(&net.prover_url).is_ok() && endpoint_url(&net.rpc_url).is_ok()
            })
        })
    }
}
