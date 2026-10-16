# Reviewed public STRK treasury transfers

Implemented in source on 2026-10-16; **not installed or live-verified**. This is a
narrow Sepolia-only native companion endpoint for an external application such
as Floodgate. Mainnet returns `MAINNET_NOT_ENABLED`; independent human security
review remains required. Existing generic wallet methods are unchanged.

## Authority and request

`companion_treasuryCapabilities {}` is public metadata. It reports schema 1,
`methods: [companion_treasuryTransfer, companion_treasuryStatus]`,
`networks: [SN_SEPOLIA]`, fixed `token`,
`intentHashAlgorithm: sha256-canonical-json-v1`, `signingEnabled` (whether its
journal opened), and `mainnetEnabled: false`. This is capability discovery, not
permission or proof of balances. Account selection remains in application setup.

`companion_treasuryTransfer` requires a valid **app** pairing and a selected
existing user-domain account. It accepts exactly these fields:

```json
{
  "operationId": "12345678-1234-1234-1234-123456789abc",
  "plan_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "chainId": "0x534e5f5345504f4c4941",
  "account_address": "0x123",
  "transfers": [{"recipient":"0x456","amount_low":"0x11","amount_high":"0x1"}],
  "nonce": "0x0",
  "resource_bounds": {
    "l1_gas": {"max_amount":"0x1","max_price_per_unit":"0x2"},
    "l2_gas": {"max_amount":"0x1","max_price_per_unit":"0x3"},
    "l1_data_gas": {"max_amount":"0x1","max_price_per_unit":"0x4"}
  },
  "max_fee": "9",
  "expires_at": 1800000000000
}
```

Example addresses/time are placeholders, not executable instructions. Hex is
lowercase and minimal (`0x0` for zero); amounts use exact uint128 low/high limbs,
not field-reduced values. Maximum 50 unique, positive, non-self transfers. Token
is fixed to STRK at
`0x4718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d`.
Unknown fields, arbitrary calls, token overrides and returned-signature modes
are rejected. Gas amounts are uint64, prices uint128; `max_fee` is a canonical
decimal uint256. The checked sum of all three resource products must fit the
cap and be positive; tip is zero. Principal sums and principal plus fee cap
must fit uint256. No fee quote or balance verification is claimed here: the
external planner must independently check balances, budgets, commitments and
fresh fee estimates. Signing binds exactly the supplied reviewed bounds.

`expires_at` is a safe-integer Unix millisecond authorization deadline no more
than five minutes ahead. It limits this wallet's approval/signing/submission
attempt, **not on-chain validity of a signature**. Once signing is possible,
expiry never means funds are safely released.

## Immutable review and hash

`intentHash = SHA256(UTF8("gravity-treasury-v1\n" + canonicalJSON(request)))`.
Canonical JSON recursively sorts object keys in ASCII lexical order, retains
array order, has no spaces, and includes every request field. The typed request
contains only ASCII strings and safe integers; no optional/defaulted fields or
floats. `plan_hash` binds the external immutable plan without implying Gravity
has independently validated that plan's budgets.

The independent Python-generated cross-language fixture is
[`treasury-intent-v1.json`](../../crates/wallet-rpc/tests/fixtures/treasury-intent-v1.json).
Its expected hash is
`03ed385903d0dfb4f104a7a3ce4cfdabc5093db438d6192047d2cc41da446c61`.

Every transfer **always** uses native approval, including clients with active
auto-approval grants. The scrollable review lists sender, chain, token, every
recipient and decimal uint256 amount, total principal, nonce, fee cap, all
resource bounds, expiry, operation ID, external plan hash and wallet intent hash.
The response never includes a signature or signed transaction.

## Durable lifecycle and recovery

The server opens `treasury.sqlite3` next to its configured pairing store. It is
a separate mode-0600 SQLite journal, using full synchronous commits and atomic
conditional transitions; it does not alter the vault. Failure to open it
disables the endpoint. The journal stores public identifiers/hashes, expiry,
status and transaction hash, not keys, pairing tokens or signed payloads.

1. Validate the exact schema/network and app caller. Reserve the operation and
   account/chain before network work or approval. Operation ID is immutable;
   conflicting valid intents atomically receive their own bound unsigned `cancelled`
   record. Storage failures remain errors, never proof of cancellation. Account/chain/nonce remains
   unique for potentially signed operations, including submitted ones.
2. Fresh node chain ID and nonce must match the request; sender must be deployed.
   Account availability and configured node failures give a bound `cancelled`
   result after reservation. Initial expired requests get bound `expired`.
3. Prompt for at most 60 seconds, within expiry. No grant bypass. Rejection or
   timeout gives `rejected` (or `expired` if its deadline passed).
4. After approval, recheck node chain and nonce. Recheck paired token, app scope,
   user account identity, current wallet chain, unlock/session epoch and expiry.
   Authorization/session locks cover the final check and synchronous signing.
5. Durably transition to `submitting` **before signing**. Persist the locally
   calculated transaction hash before the one-shot broadcast. Any crash, error,
   timeout, mismatched node hash or lost response after this boundary stays
   `submitting`/`unknown`. There is no signature replay or automatic rebroadcast.
6. A matching broadcast response gives `submitted`; this is **not an accepted
   receipt**. Independently reconcile receipt, chain, calldata, token events,
   principal and fees before posting funding credits in the external ledger.

`companion_treasuryStatus {operationId}` requires the owning pairing and returns:
`{schemaVersion:1, operationId, intentHash, plan_hash, status, transactionHash}`.
A repeat transfer with the identical request returns that record without a new
prompt or broadcast. A changed intent under the same ID fails. Unknown IDs
remain unknown; an HTTP/RPC error alone is not proof of no signing.

Only durable `rejected`, `expired` and `cancelled` records with null transaction
hash prove this endpoint never crossed its signing boundary. A crashed
`awaiting_approval` record can atomically expire through status lookup; its
compare-and-swap cannot expire a concurrent `submitting` record. Dropping a
connection or revoking authorization after signing cannot revoke a transaction.
Potentially signed unresolved records conservatively retain their account lock.
An explicit status call can resolve `unknown`/`submitting` with a stored hash to
`submitted` only after read-only configured-node evidence: Sepolia chain before
and after, exact receipt hash, accepted finality, succeeded **or reverted**
execution, exact INVOKE v3 transaction hash/sender/nonce, and membership in the
canonical block queried by number with matching receipt block hash/number.
The atomic update binds the original owner, ID and stored hash. Account lock
is then released, but that consumed nonce remains reserved. Reverted acceptance
never establishes successful principal funding. Missing hash, pending/not-found,
wrong chain, inconsistent inclusion and unavailable evidence remain held.
Concurrent status checks are idempotent; a racing broadcast completion may return
a transition error, requiring another status read, never a second submission.
There is no blind unlock or ledger-release endpoint. These are configured-provider
observations, not a cryptographic light-client proof or guarantee against later
reorganization. The external ledger independently reconciles amounts and fees.

Recovery reads have a combined 15-second deadline, bounded 2 MiB JSON responses,
strict response IDs/envelopes, and no redirect or retry. The response schema is
unchanged. An identical repeated transfer only reads the local journal.

Network work is bounded to 15 seconds before approval, 15 seconds after approval,
and 30 seconds for a single broadcast. Read-only checks may fail closed; upstream
error details are never returned. Do not mistake these checks for an independent
RPC operator trust guarantee, account-class audit, fee estimate or on-chain expiry.

## Primary RPC schema reference

Checked against Starknet OpenRPC **0.10.4**, source commit
[`1edf5420ff8a252e1f8e56725ef85ab9b63eb2e4`](https://github.com/starkware-libs/starknet-specs/blob/1edf5420ff8a252e1f8e56725ef85ab9b63eb2e4/api/starknet_api_openrpc.json):
`starknet_getTransactionReceipt` / `TXN_RECEIPT_WITH_BLOCK_INFO`,
`INVOKE_TXN_RECEIPT` / `COMMON_RECEIPT_PROPERTIES`,
`starknet_getTransactionByHash` / `INVOKE_TXN_V3`, and
`starknet_getBlockWithTxHashes` / `BLOCK_WITH_TX_HASHES`.
Pre-confirmed receipts can omit block hash; recovery deliberately rejects them.
The parser verifies only the listed recovery-critical fields, not every unrelated
schema field. Local synthetic HTTP tests exercise the actual read transport;
no live node or installed-wallet compatibility claim follows.

## Verification and review gates

Tests use the public `abandon … about` mnemonic, temporary journals and mock
nodes. They cover grants, exact review, field/hash binding, uint256 arithmetic,
wrong fee/chain/nonce, revocation, lock/session replacement, rejection/timeout,
expiry, concurrent intents, durable restart recovery and uncertain responses.
They do not establish live Sepolia acceptance, deployment compatibility of the
installed app or mainnet readiness. No app installation, pairing, live signing,
funded transfer, address migration, viewing-key change or vault-format change
was performed for this feature.
