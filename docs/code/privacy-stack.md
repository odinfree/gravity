# gravity and the Starknet privacy stack

## Layers and current support

`gravity` is the wallet and agent approval boundary. A pool adapter constructs
private actions; a selected proving service generates proofs; a discovery service
finds notes; the public Starknet node estimates and broadcasts final transactions.
These are separate roles. A working node or prover health check does not establish
that a pool accepts a deposit or that a screening partner has authorized it.

The first adapter is `privacy/adapters/strk20-v2.mjs`, using SDK
`0.14.3-rc.8`. Its current asset is STRK. It implements register, deposit (shield),
private transfer and withdraw (unshield), with live pool fees and proof-validity
reads. Compatible deployments may use different pool addresses and deposit
policies. A pool with another ABI, asset model or proof format needs a new adapter
and corresponding Rust validation; setting an arbitrary address is insufficient.

The native Privacy page and authenticated `companion_privacy*` extension are
implemented. This does not claim the deferred standard `wallet_strk20*` API.

## Configuration

The desktop stores per-network pool address, discovery URL and deposit policy in
its private `privacy/settings.json`. RPC and prover URLs come from Settings.
Credentials are IPC-only and are not exposed by `GET /` or agent methods.

| Setting | Purpose |
|---|---|
| Blockchain RPC | Chain reads, final public-call estimation and submission |
| `starknet-rpc` prover | Your Starknet transaction prover, often `http://127.0.0.1:3000` through SSH |
| Discovery endpoint | Your compatible indexer, often `http://127.0.0.1:8080` |
| Pool address | The deployed pool for the selected network and adapter |
| `required` policy | Deposit preparation requires the prover's screening attestation |
| `pool_enforced` policy | Custom compatible deployment; its contract enforces its policy, no blanket attestation assumption |

The shipped mainnet/testnet pool addresses default to `required` and reject an
attempt to disable that policy. A local policy setting cannot change a contract's
rules. Other compliance protocols, attestation schemas, allowlists or verifiers
need explicit implementation and review.

The default pool does not currently offer out-of-the-box shielding through
gravity. Its screening integration is an outstanding wallet/operator dependency;
end users should not be asked to obtain partner credentials. The official
[proof interceptor](https://github.com/starkware-libs/starknet-privacy/blob/main/proof-interceptor/README.md)
needs a screening URL and operator-issued partner credentials. Running the
interceptor without them, or checking its health endpoint, does not supply an
attestation. gravity does not route private inputs to an unconfigured service.

`deposit_screening` reports `unverified`, `signature_missing` or `pool_enforced`.
After a deposit proof lacks its required signature, a private diagnostic remembers
the failure across restart and disables repeated shielding attempts in the UI.
It stores only a hash of the service/pool/network configuration, a timestamp and
the missing-signature flag. It is not an authorization or a transaction journal.
Changing the configuration invalidates the observation; saving Privacy services
also clears it after an operator fixes a service at the same endpoint. Every
deposit still checks its own attestation, including calls through the agent API.

Use a prover and discovery service configured for the same chain and pool.
Only HTTPS or loopback HTTP is accepted; use SSH for a remote self-hosted node.
Read [prover configuration](self-hosted-prover.md). Node.js 24+ must be available at
`/opt/homebrew/bin/node`, `/usr/local/bin/node` or `/usr/bin/node`, or set
`STRKD_PRIVACY_NODE` to its absolute path before launching the app.

## First-use flow

1. Fund and deploy a newly created account using the wallet's normal account flow.
2. Check its privacy registration. If an existing pool viewing key differs from
   gravity's derived key, stop and restore the original privacy wallet. gravity
   will not overwrite that registration or claim to recover another wallet's key.
3. Click **Register** and approve the amount/fees once. gravity prepares the
   proof, submits after approval, polls the receipt and updates registration.
4. Require `SUCCEEDED` and `ACCEPTED_ON_L2`/`ACCEPTED_ON_L1`, and wait until the
   receipt block is older than `head - 10` before building a dependent proof.
5. Click the next action; one final wallet approval follows proof preparation. For a private transfer the recipient must already be
   registered. A withdrawal defaults to this account if no recipient is entered.
6. Check the receipt and discover notes after acceptance. Fresh notes are excluded
   from the mature spendable balance. The current selector may consolidate all
   mature notes for the token; it is not a coin-selection privacy optimizer.

The known screened pools need a partner-operated screening integration attached
to the prover. Operator-issued screening URL/partner credentials belong in that
service, never Git or the frontend. A missing attestation blocks the deposit;
registration/discovery and other actions follow the pool's own rules. No live
screening credential or shared hosted prover is bundled.

## Keys, signing and private inputs

The seed and signing keys remain in Rust. `wallet-core::privacy::viewing_key_v1`
derives a separate viewing scalar using hardened account branches `0x5354524b`
(USER) / `0x5354524c` (AGENT), coin type 9004 and the account index. HKDF-SHA256 with
salt `strkd/strk20/viewing-key/v1` and chain bytes, a zero separator and the 32-byte
pool address produces a 31-byte scalar. The all-zero result maps to one. This
version and its legacy salt must never change silently after registration.
A public BIP-39 fixture pins a recovery vector. This is a new, unaudited derivation;
it is not a claim of recovery interoperability with another wallet.

A private Node child receives the viewing key through anonymous stdin, not
process arguments, environment variables or the public loopback API. Its runtime
is `privacy/worker.mjs`; no separate SDK HTTP server is started. Rust answers a
single constrained virtual-signing request and verifies that the proving payload
matches the signed invocation. The virtual sender is the pool, with zero effective
resource prices and `tip=0`. Only the final public calls/proof go to the node for
fee estimation. Public agents never receive private compile calldata or keys.

The worker exits after each operation. A lock/session change cancels the child
and invalidates reviews; an already-dispatched remote proof cannot be recalled.
Zeroizing Rust buffers reduces retention but does not securely erase all copies
made by JavaScript, serialization, the OS, or external services.

The prover and discovery service receive private/viewing material and are trusted
operators. A local tunnel encrypts transit, not the remote operator's access.
A public RPC sees the final submitting account, fees, proof and public pool
outputs. Deposits and withdrawals reveal their amount; withdrawals also reveal
the recipient. This integration does not promise unlinkability of the fee payer.

## Reviews, receipts and recovery

Preparation does not broadcast. It produces a client/account/network/session/
configuration-bound review with a five-minute lifetime. Final spending always
requests human approval, even under a generic permission grant. Rust checks the
exact STRK allowance (pool fee plus deposit, or only the fee for other actions),
review metadata, nonce, current pool fee, proof age and resource-bound cap.

A review is consumed once. Before the one permitted broadcast, gravity stores the
deterministically calculated transaction hash in an owner-only journal. Transport
or ambiguous RPC failure becomes `submission_unknown`; it is not retried. An
unresolved journal entry blocks further privacy preparation for that account and
chain. **Recent privacy transactions** restores receipt checks after restart.
Only a final accepted receipt clears that gate. If a node definitively rejected
an unknown transaction and never provides a receipt, manual reconciliation is
required; absence of a receipt alone is not proof that it was never submitted.
Do not erase an unknown record and resubmit blindly.

Final receipts remain final when discovery is unavailable. The UI refreshes
registration separately and reports a balance-refresh failure without restarting
receipt polling. Balance discovery pins a numeric chain head; notes newer than
the mature proof base are excluded from the spendable amount. Read-only worker
requests time out after 60 seconds; proof preparation retains its 16-minute limit.

Journals store hashes, public accounts and receipt states, not private notes.
Privacy RPC requests/results/errors are excluded from full-payload request logs.
The `starknet-rpc` prover backend stores proof output and sanitized metadata, not
the private proving request. Protect the local app-data directory and backups.

## Agent contract

Pair normally. Each request uses explicit `account` and `chain_id`, with amounts
as decimal strings of STRK base units (`10 STRK = "10000000000000000000"`).

| Method | Result / approval |
|---|---|
| `companion_privacyStatus` | Registration, public balance, fee and pool policy; read-only |
| `companion_privacyBalances` | Shielded and mature spendable balances; always prompts |
| `companion_privacyPrepare` | `{operation: register\|deposit\|transfer\|withdraw, amount?, recipient?}` plus account/chain; prompts, proves and returns public review |
| `companion_privacySubmit` | `{review_id}`; always prompts, single broadcast, returns hash |
| `companion_privacyReceipt` | `{transaction_hash, chain_id}`; read-only reconciliation |
| `companion_privacyHistory` | `{account, chain_id}`; scoped local journal |

A generic agent cannot change service URLs, pool policy or derivation via these
methods. New pool adapters must preserve scope, policy review, private-input
routing, proof binding, exact allowances and no-retry receipt recovery. Add
versioned recovery vectors and positive/negative wire tests before enabling them.

## Validation boundary

The automated pipeline runs the shipped SDK worker, real Rust signing and a
synthetic chain/prover, then checks review ownership, duplicate submission
prevention and receipt reconciliation. It verifies integration behavior, not a
cryptographic proof or live deposit acceptance. The adapter's crypto path requires
independent human review; live shield/transfer/withdraw receipt and note tests
remain outstanding. See [status](../project/status.md).
