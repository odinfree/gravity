//! Narrow public STRK treasury review. No arbitrary calls, grants or returned signatures.
//! Durable transitions fail closed: an interrupted reservation never automatically retries.
use crate::{
    approval::{ApprovalRequest, Decision},
    auth::{ClientKind, PairedClient},
    dispatch::ServerState,
    error::WalletRpcError,
    log::now_unix_ms,
    node::FeeBounds,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, sync::Mutex, time::Duration};
use wallet_core::{AccountRef, Call, ChainId, Felt, InvokeV3Params, ResourceBounds};

pub const TOKEN: &str = "0x4718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d";
fn bad(s: &str) -> WalletRpcError {
    WalletRpcError::InvalidRequest(s.into())
}
fn storage(_: rusqlite::Error) -> WalletRpcError {
    WalletRpcError::Precondition("Treasury journal unavailable; reconcile before retry".into())
}
fn fh(f: Felt) -> String {
    format!("0x{f:x}")
}
fn hex_felt(s: &str) -> Result<Felt, WalletRpcError> {
    if s.len() > 66 || !s.starts_with("0x") {
        return Err(bad("Expected canonical hex"));
    }
    let f = Felt::from_hex(s).map_err(|_| bad("Invalid felt"))?;
    if fh(f) != s {
        return Err(bad("Expected canonical hex"));
    }
    Ok(f)
}
fn address(s: &str) -> Result<Felt, WalletRpcError> {
    let f = hex_felt(s)?;
    let bound = Felt::from_hex("0x7ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff00")
        .unwrap();
    if f == Felt::ZERO || f >= bound {
        return Err(bad("Invalid address"));
    }
    Ok(f)
}
fn limb(s: &str) -> Result<u128, WalletRpcError> {
    hex_felt(s)?;
    u128::from_str_radix(&s[2..], 16).map_err(|_| bad("Expected uint128 hex"))
}
// Checked unsigned 256-bit arithmetic, without reduction modulo Starknet's field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct U256([u64; 4]);
impl U256 {
    fn pair(lo: u128, hi: u128) -> Self {
        Self([lo as u64, (lo >> 64) as u64, hi as u64, (hi >> 64) as u64])
    }
    fn add(self, b: Self) -> Result<Self, WalletRpcError> {
        let mut a = self.0;
        let mut carry = 0u128;
        for (i, x) in a.iter_mut().enumerate() {
            let v = *x as u128 + b.0[i] as u128 + carry;
            *x = v as u64;
            carry = v >> 64;
        }
        if carry != 0 {
            return Err(bad("uint256 total overflow"));
        }
        Ok(Self(a))
    }
    fn mul(self, b: u64) -> Result<Self, WalletRpcError> {
        let mut a = self.0;
        let mut carry = 0u128;
        for x in &mut a {
            let v = *x as u128 * b as u128 + carry;
            *x = v as u64;
            carry = v >> 64;
        }
        if carry != 0 {
            return Err(bad("uint256 overflow"));
        }
        Ok(Self(a))
    }
    fn decimal(s: &str) -> Result<Self, WalletRpcError> {
        if s.is_empty()
            || s.len() > 78
            || !s.bytes().all(|b| b.is_ascii_digit())
            || (s.len() > 1 && s.starts_with('0'))
        {
            return Err(bad("Expected canonical decimal uint256"));
        }
        let mut n = Self([0; 4]);
        for b in s.bytes() {
            n = n.mul(10)?.add(Self::pair((b - b'0') as u128, 0))?;
        }
        Ok(n)
    }
    fn dec(self) -> String {
        let mut a = self.0;
        let mut out = Vec::new();
        while a != [0; 4] {
            let mut rem = 0u128;
            for x in a.iter_mut().rev() {
                let n = (rem << 64) | *x as u128;
                *x = (n / 10) as u64;
                rem = n % 10;
            }
            out.push(b'0' + rem as u8);
        }
        if out.is_empty() {
            out.push(b'0');
        }
        out.reverse();
        String::from_utf8(out).unwrap()
    }
    fn exceeds(self, b: Self) -> bool {
        self.0.iter().rev().cmp(b.0.iter().rev()).is_gt()
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Transfer {
    pub recipient: String,
    pub amount_low: String,
    pub amount_high: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bound {
    pub max_amount: String,
    pub max_price_per_unit: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub l1_gas: Bound,
    pub l2_gas: Bound,
    pub l1_data_gas: Bound,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransferRequest {
    #[serde(rename = "operationId")]
    pub operation_id: String,
    pub plan_hash: String,
    #[serde(rename = "chainId")]
    pub chain_id: String,
    pub account_address: String,
    pub transfers: Vec<Transfer>,
    pub nonce: String,
    pub resource_bounds: Bounds,
    pub max_fee: String,
    pub expires_at: u64,
}
fn sorted(v: Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, v)| (k, sorted(v)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.into_iter().map(sorted).collect()),
        x => x,
    }
}
pub fn intent_hash(req: &TransferRequest) -> String {
    let bytes = serde_json::to_vec(&sorted(serde_json::to_value(req).unwrap())).unwrap();
    let mut h = Sha256::new();
    h.update(b"gravity-treasury-v1\n");
    h.update(bytes);
    hex::encode(h.finalize())
}
fn uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
            }
        })
}
impl TransferRequest {
    fn validate(&self) -> Result<(Vec<Call>, FeeBounds, U256), WalletRpcError> {
        if !uuid(&self.operation_id)
            || self.plan_hash.len() != 64
            || !self
                .plan_hash
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(bad("Invalid operationId or plan_hash"));
        }
        if self.chain_id == fh(ChainId::Mainnet.as_felt()) {
            return Err(WalletRpcError::Precondition("MAINNET_NOT_ENABLED".into()));
        }
        if self.chain_id != fh(ChainId::Sepolia.as_felt()) {
            return Err(WalletRpcError::ChainIdNotSupported);
        }
        address(&self.account_address)?;
        hex_felt(&self.nonce)?;
        if self.expires_at > 9_007_199_254_740_991 {
            return Err(bad("Unsafe expiry integer"));
        }
        if self.transfers.is_empty() || self.transfers.len() > 50 {
            return Err(bad("Expected 1..50 transfers"));
        }
        let mut seen = std::collections::HashSet::new();
        let mut total = U256([0; 4]);
        let mut calls = Vec::new();
        for t in &self.transfers {
            let to = address(&t.recipient)?;
            let lo = limb(&t.amount_low)?;
            let hi = limb(&t.amount_high)?;
            if t.recipient == self.account_address
                || !seen.insert(t.recipient.clone())
                || (lo == 0 && hi == 0)
            {
                return Err(bad("Duplicate/self recipient or zero amount"));
            }
            total = total.add(U256::pair(lo, hi))?;
            calls.push(Call {
                to: address(TOKEN)?,
                selector: wallet_core::get_selector_from_name("transfer"),
                calldata: vec![to, Felt::from(lo), Felt::from(hi)],
            });
        }
        let bound = |b: &Bound| -> Result<ResourceBounds, WalletRpcError> {
            Ok(ResourceBounds {
                max_amount: u64::try_from(limb(&b.max_amount)?)
                    .map_err(|_| bad("Gas amount exceeds uint64"))?,
                max_price_per_unit: limb(&b.max_price_per_unit)?,
            })
        };
        let bounds = FeeBounds {
            l1_gas: bound(&self.resource_bounds.l1_gas)?,
            l2_gas: bound(&self.resource_bounds.l2_gas)?,
            l1_data_gas: bound(&self.resource_bounds.l1_data_gas)?,
        };
        let max = U256::decimal(&self.max_fee)?;
        let mut fee = U256([0; 4]);
        for b in [bounds.l1_gas, bounds.l2_gas, bounds.l1_data_gas] {
            fee = fee.add(U256::pair(b.max_price_per_unit, 0).mul(b.max_amount)?)?;
        }
        if fee == U256([0; 4]) || fee.exceeds(max) {
            return Err(bad("Resource fee exceeds max_fee or is zero"));
        }
        total.add(max)?;
        Ok((calls, bounds, total))
    }
    fn summary(&self, total: U256, intent: &str) -> String {
        let mut lines = vec![
            "PUBLIC STRK TREASURY TRANSFER — SIGN AND SUBMIT ONCE".into(),
            format!("Sender: {}", self.account_address),
            format!("Chain: SN_SEPOLIA ({})", self.chain_id),
            format!("Token: {TOKEN} (18 decimals; exact base units below)"),
        ];
        for (i, t) in self.transfers.iter().enumerate() {
            lines.push(format!(
                "{}. Recipient: {}\n   Amount: {} STRK base units",
                i + 1,
                t.recipient,
                U256::pair(limb(&t.amount_low).unwrap(), limb(&t.amount_high).unwrap()).dec()
            ));
        }
        lines.extend([
            format!("Total principal: {} STRK base units", total.dec()),
            format!("Nonce: {}", self.nonce),
            format!("Maximum fee: {} STRK base units; tip: 0", self.max_fee),
        ]);
        for (name, b) in [
            ("L1 gas", &self.resource_bounds.l1_gas),
            ("L2 gas", &self.resource_bounds.l2_gas),
            ("L1 data gas", &self.resource_bounds.l1_data_gas),
        ] {
            lines.push(format!(
                "{name}: max_amount={} × max_price_per_unit={} FRI",
                b.max_amount, b.max_price_per_unit
            ));
        }
        lines.extend([format!("Expires at Unix milliseconds: {}",self.expires_at),format!("Operation: {}",self.operation_id),format!("Floodgate plan: {}",self.plan_hash),format!("Intent SHA256: {intent}"),"This authorization submits the reviewed transfers. A broadcast response is not receipt acceptance. No automatic retry after uncertainty.".into()]);
        lines.join("\n")
    }
}

pub struct TreasuryJournal(Mutex<Connection>);
impl TreasuryJournal {
    pub fn open(path: &Path) -> Result<Self, WalletRpcError> {
        // Exclusive initial creation with private permissions; never touch vault files.
        if !path.exists() {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(path) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(bad("Cannot create treasury journal")),
            }
        }
        let meta =
            std::fs::symlink_metadata(path).map_err(|_| bad("Cannot inspect treasury journal"))?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(bad("Treasury journal must be a regular file"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                return Err(bad("Treasury journal requires mode 0600"));
            }
        }
        let db = Connection::open(path).map_err(storage)?;
        db.busy_timeout(Duration::from_secs(2)).map_err(storage)?;
        db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS treasury_ops(id TEXT PRIMARY KEY,owner TEXT NOT NULL,intent TEXT NOT NULL,plan TEXT NOT NULL,chain TEXT NOT NULL,sender TEXT NOT NULL,nonce TEXT NOT NULL,status TEXT NOT NULL,txhash TEXT,expires INTEGER NOT NULL); CREATE UNIQUE INDEX IF NOT EXISTS treasury_active_account ON treasury_ops(chain,sender) WHERE status NOT IN ('rejected','expired','cancelled','submitted'); CREATE UNIQUE INDEX IF NOT EXISTS treasury_nonce ON treasury_ops(chain,sender,nonce) WHERE status NOT IN ('rejected','expired','cancelled');").map_err(storage)?;
        Ok(Self(Mutex::new(db)))
    }
    fn get(&self, id: &str, owner: &str) -> Result<Option<Value>, WalletRpcError> {
        let db = self
            .0
            .lock()
            .map_err(|_| bad("Treasury journal poisoned"))?;
        db.execute("UPDATE treasury_ops SET status='expired' WHERE id=? AND owner=? AND status='awaiting_approval' AND txhash IS NULL AND expires<=?",params![id,owner,now_unix_ms()]).map_err(storage)?;
        db.query_row("SELECT intent,plan,status,txhash FROM treasury_ops WHERE id=? AND owner=?",params![id,owner],|r|Ok(json!({"schemaVersion":1,"operationId":id,"intentHash":r.get::<_,String>(0)?,"plan_hash":r.get::<_,String>(1)?,"status":r.get::<_,String>(2)?,"transactionHash":r.get::<_,Option<String>>(3)?}))).optional().map_err(storage)
    }
    fn reserve(
        &self,
        r: &TransferRequest,
        owner: &str,
        intent: &str,
    ) -> Result<(), WalletRpcError> {
        let db = self
            .0
            .lock()
            .map_err(|_| bad("Treasury journal poisoned"))?;
        // One SQLite statement atomically reserves or records a safe unsigned conflict.
        // Generic storage/ID errors never become a terminal cancellation response.
        db.execute(
            "INSERT INTO treasury_ops SELECT ?1,?2,?3,?4,?5,?6,?7,CASE WHEN EXISTS(SELECT 1 FROM treasury_ops WHERE chain=?5 AND sender=?6 AND (status NOT IN ('rejected','expired','cancelled','submitted') OR (nonce=?7 AND status NOT IN ('rejected','expired','cancelled')))) THEN 'cancelled' ELSE 'awaiting_approval' END,NULL,?8",
            params![r.operation_id,owner,intent,r.plan_hash,r.chain_id,r.account_address,r.nonce,r.expires_at],
        ).map_err(storage)?;
        Ok(())
    }
    fn recovery(
        &self,
        id: &str,
        owner: &str,
    ) -> Result<Option<(String, String, String)>, WalletRpcError> {
        let db = self
            .0
            .lock()
            .map_err(|_| bad("Treasury journal poisoned"))?;
        db.query_row("SELECT txhash,sender,nonce FROM treasury_ops WHERE id=? AND owner=? AND chain='0x534e5f5345504f4c4941' AND status IN ('unknown','submitting') AND txhash IS NOT NULL",params![id,owner],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(storage)
    }
    fn accepted(&self, id: &str, owner: &str, hash: &str) -> Result<(), WalletRpcError> {
        let db = self
            .0
            .lock()
            .map_err(|_| bad("Treasury journal poisoned"))?;
        // Race-safe with the original broadcast completion: only this exact persisted hash.
        db.execute("UPDATE treasury_ops SET status='submitted' WHERE id=? AND owner=? AND txhash=? AND status IN ('unknown','submitting')",params![id,owner,hash]).map_err(storage)?;
        Ok(())
    }
    fn transition(
        &self,
        id: &str,
        from: &str,
        to: &str,
        hash: Option<&str>,
    ) -> Result<(), WalletRpcError> {
        let db = self
            .0
            .lock()
            .map_err(|_| bad("Treasury journal poisoned"))?;
        let n=db.execute("UPDATE treasury_ops SET status=?,txhash=COALESCE(?,txhash) WHERE id=? AND status=? AND (? != 'awaiting_approval' OR ? != 'submitting' OR expires > ?)",params![to,hash,id,from,from,to,now_unix_ms()]).map_err(storage)?;
        if n != 1 {
            return Err(bad("Treasury transition conflict; reconcile"));
        }
        Ok(())
    }
}
fn journal(state: &ServerState) -> Result<&TreasuryJournal, WalletRpcError> {
    state.treasury.as_deref().ok_or_else(|| {
        WalletRpcError::Precondition("Durable treasury journal is not configured".into())
    })
}
pub fn capabilities(state: &ServerState) -> Value {
    json!({"schemaVersion":1,"methods":["companion_treasuryTransfer","companion_treasuryStatus"],"networks":["SN_SEPOLIA"],"token":TOKEN,"intentHashAlgorithm":"sha256-canonical-json-v1","signingEnabled":state.treasury.is_some(),"mainnetEnabled":false})
}
pub fn status(
    state: &ServerState,
    client: &PairedClient,
    p: &Value,
) -> Result<Value, WalletRpcError> {
    let o = p.as_object().ok_or_else(|| bad("Expected operationId"))?;
    let id = o
        .get("operationId")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("Expected operationId"))?;
    if o.len() != 1 || !uuid(id) {
        return Err(bad("Expected operationId only"));
    }
    journal(state)?
        .get(id, &client.id)?
        .ok_or_else(|| bad("Unknown treasury operation"))
}
/// Read-only chain recovery. A verified accepted original hash consumes its nonce;
/// it clears the account lock without claiming funding or ever replaying a signature.
pub async fn recover_status(
    state: &ServerState,
    client: &PairedClient,
    p: &Value,
) -> Result<Value, WalletRpcError> {
    let initial = status(state, client, p)?;
    let id = initial["operationId"]
        .as_str()
        .ok_or_else(|| bad("Missing operation ID"))?;
    let j = journal(state)?;
    if let Some((hash, sender, nonce)) = j.recovery(id, &client.id)? {
        if let Some(node) = state.node_for(ChainId::Sepolia) {
            let check = async {
                if node.chain_id().await.ok() != Some(ChainId::Sepolia.as_felt()) {
                    return false;
                }
                let (Ok(h), Ok(s), Ok(n)) = (hex_felt(&hash), address(&sender), hex_felt(&nonce))
                else {
                    return false;
                };
                if !node.accepted_invoke(&h, &s, &n).await.unwrap_or(false) {
                    return false;
                }
                node.chain_id().await.ok() == Some(ChainId::Sepolia.as_felt())
            };
            if matches!(
                tokio::time::timeout(Duration::from_secs(15), check).await,
                Ok(true)
            ) {
                j.accepted(id, &client.id, &hash)?;
            }
        }
    }
    status(state, client, p)
}

async fn account(
    state: &ServerState,
    client: &PairedClient,
    token: &str,
    r: &TransferRequest,
) -> Result<AccountRef, WalletRpcError> {
    let clients = state.clients.lock().await;
    let current = clients.verify(token).ok_or(WalletRpcError::NotRegistered)?;
    if current.id != client.id || current.kind != ClientKind::App {
        return Err(WalletRpcError::Forbidden);
    }
    let session = state.session.lock().await;
    if session.chain() != ChainId::Sepolia {
        return Err(WalletRpcError::ChainIdNotSupported);
    }
    let result = session
        .registry()?
        .user_accounts()
        .find(|a| Felt::from_hex(&a.address).ok() == Felt::from_hex(&r.account_address).ok())
        .cloned()
        .ok_or(WalletRpcError::Forbidden);
    result
}
pub async fn transfer(
    state: &ServerState,
    client: &PairedClient,
    token: &str,
    p: &Value,
) -> Result<Value, WalletRpcError> {
    let r: TransferRequest =
        serde_json::from_value(p.clone()).map_err(|_| bad("Invalid treasury schema"))?;
    let (calls, bounds, total) = r.validate()?;
    let intent = intent_hash(&r);
    let j = journal(state)?;
    if let Some(previous) = j.get(&r.operation_id, &client.id)? {
        if previous["intentHash"] != intent {
            return Err(bad("Operation intent was modified"));
        }
        return Ok(previous);
    }
    let now = now_unix_ms();
    if r.expires_at > now + 300_000 {
        return Err(bad("Expiry must be within the next five minutes"));
    }
    if client.kind != ClientKind::App {
        return Err(WalletRpcError::Forbidden);
    }
    let sender = address(&r.account_address)?;
    let nonce = hex_felt(&r.nonce)?;
    // Freeze both account and nonce before any network await/prompt; another intent cannot race.
    j.reserve(&r, &client.id, &intent)?;
    let reserved = status(state, client, &json!({"operationId":r.operation_id}))?;
    if reserved["status"] != "awaiting_approval" {
        return Ok(reserved);
    }
    if r.expires_at <= now_unix_ms() {
        j.transition(&r.operation_id, "awaiting_approval", "expired", None)?;
        return status(state, client, &json!({"operationId":r.operation_id}));
    }
    let original = match account(state, client, token, &r).await {
        Ok(a) => a,
        Err(_) => {
            j.transition(&r.operation_id, "awaiting_approval", "cancelled", None)?;
            return status(state, client, &json!({"operationId":r.operation_id}));
        }
    };
    let session_epoch = state.session.lock().await.epoch();
    let node = match state.node_for(ChainId::Sepolia) {
        Some(n) => n,
        None => {
            j.transition(&r.operation_id, "awaiting_approval", "cancelled", None)?;
            return status(state, client, &json!({"operationId":r.operation_id}));
        }
    };
    let preflight = async {
        if node
            .chain_id()
            .await
            .map_err(|_| bad("Treasury node chain unavailable"))?
            != ChainId::Sepolia.as_felt()
        {
            return Err(bad("Treasury node chain mismatch"));
        }
        if node
            .get_nonce(&sender)
            .await
            .map_err(|_| bad("Treasury nonce unavailable"))?
            != nonce
        {
            return Err(bad("Treasury nonce changed"));
        }
        if !node
            .is_deployed(&sender)
            .await
            .map_err(|_| bad("Treasury deployment unavailable"))?
        {
            return Err(bad("Treasury account is not deployed"));
        }
        Ok::<(), WalletRpcError>(())
    };
    if !matches!(
        tokio::time::timeout(Duration::from_secs(15), preflight).await,
        Ok(Ok(()))
    ) {
        j.transition(&r.operation_id, "awaiting_approval", "cancelled", None)?;
        return status(state, client, &json!({"operationId":r.operation_id}));
    }
    if r.expires_at <= now_unix_ms() {
        j.transition(&r.operation_id, "awaiting_approval", "expired", None)?;
        return status(state, client, &json!({"operationId":r.operation_id}));
    }
    let wait_ms = r.expires_at.saturating_sub(now_unix_ms()).min(60_000);
    let decision = tokio::time::timeout(
        Duration::from_millis(wait_ms),
        state.approver.request_approval(ApprovalRequest {
            client_label: client.label.clone(),
            method: "companion_treasuryTransfer".into(),
            summary: r.summary(total, &intent),
        }),
    )
    .await;
    if !matches!(decision, Ok(Decision::Approve)) {
        let terminal = if r.expires_at <= now_unix_ms() {
            "expired"
        } else {
            "rejected"
        };
        j.transition(&r.operation_id, "awaiting_approval", terminal, None)?;
        return status(state, client, &json!({"operationId":r.operation_id}));
    }
    let recheck = async {
        if node
            .chain_id()
            .await
            .map_err(|_| bad("Treasury node unavailable"))?
            != ChainId::Sepolia.as_felt()
            || node
                .get_nonce(&sender)
                .await
                .map_err(|_| bad("Treasury nonce unavailable"))?
                != nonce
        {
            return Err(bad("Treasury chain or nonce changed"));
        }
        if account(state, client, token, &r).await? != original {
            return Err(WalletRpcError::Forbidden);
        }
        Ok::<(), WalletRpcError>(())
    };
    if !matches!(
        tokio::time::timeout(Duration::from_secs(15), recheck).await,
        Ok(Ok(()))
    ) {
        j.transition(&r.operation_id, "awaiting_approval", "cancelled", None)?;
        return status(state, client, &json!({"operationId":r.operation_id}));
    }
    // Hold authorization/session locks through the synchronous signing boundary, closing the
    // check/sign race. Never hold them while waiting for the user or node broadcast.
    let clients = state.clients.lock().await;
    let session = state.session.lock().await;
    let still_authorized = clients
        .verify(token)
        .is_some_and(|c| c.id == client.id && c.kind == ClientKind::App)
        && session.chain() == ChainId::Sepolia
        && session.epoch() == session_epoch
        && session
            .registry()
            .is_ok_and(|reg| reg.user_accounts().any(|a| a == &original));
    if !still_authorized || r.expires_at <= now_unix_ms() {
        let terminal = if r.expires_at <= now_unix_ms() {
            "expired"
        } else {
            "cancelled"
        };
        j.transition(&r.operation_id, "awaiting_approval", terminal, None)?;
        return j
            .get(&r.operation_id, &client.id)?
            .ok_or_else(|| bad("Journal missing"));
    }
    // Crash or signing error after this durable marker is UNCERTAIN, never safely released.
    j.transition(&r.operation_id, "awaiting_approval", "submitting", None)?;
    let signed = session.sign_invoke_for(
        &original,
        &calls,
        ChainId::Sepolia,
        &InvokeV3Params {
            nonce,
            tip: 0,
            l1_gas: bounds.l1_gas,
            l2_gas: bounds.l2_gas,
            l1_data_gas: bounds.l1_data_gas,
            ..Default::default()
        },
    );
    drop(session);
    drop(clients);
    let signed = match signed {
        Ok(s) => s,
        Err(_) => {
            j.transition(&r.operation_id, "submitting", "unknown", None)?;
            return status(state, client, &json!({"operationId":r.operation_id}));
        }
    };
    let txhash = fh(signed.transaction_hash);
    j.transition(&r.operation_id, "submitting", "submitting", Some(&txhash))?;
    // A known hash is persisted before the one-shot network call; no signed bytes escape.
    if now_unix_ms() >= r.expires_at {
        j.transition(&r.operation_id, "submitting", "unknown", Some(&txhash))?;
        return status(state, client, &json!({"operationId":r.operation_id}));
    }
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        node.add_invoke(
            &sender,
            &signed.calldata,
            &[signed.r, signed.s],
            &nonce,
            &bounds,
            &[],
            None,
        ),
    )
    .await;
    let final_status = if matches!(result,Ok(Ok(h)) if h==signed.transaction_hash) {
        "submitted"
    } else {
        "unknown"
    };
    j.transition(&r.operation_id, "submitting", final_status, Some(&txhash))?;
    status(state, client, &json!({"operationId":r.operation_id}))
}

#[cfg(test)]
mod arithmetic_tests {
    use super::*;
    #[test]
    fn exact_uint256_and_fee_products_never_wrap_or_round() {
        let max = "115792089237316195423570985008687907853269984665640564039457584007913129639935";
        let n = U256::decimal(max).unwrap();
        assert_eq!(n.dec(), max);
        assert_eq!(n, U256::pair(u128::MAX, u128::MAX));
        assert!(n.add(U256::pair(1, 0)).is_err());
        assert!(n.mul(2).is_err());
        assert_eq!(
            U256::pair(u128::MAX, 0).mul(u64::MAX).unwrap().dec(),
            "6277101735386680763495507056286727952620534092958556749825"
        );
        for s in [
            "",
            "01",
            "-1",
            "1e18",
            "115792089237316195423570985008687907853269984665640564039457584007913129639936",
        ] {
            assert!(U256::decimal(s).is_err());
        }
    }
}
