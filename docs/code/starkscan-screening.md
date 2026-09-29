# Starkscan screened deposits

gravity can route **mainnet deposits only** through Starkscan's operator-issued
hosted prover. Registration, private transfers and withdrawals retain the prover
selected in Settings. Note discovery retains its own service. This integration
does not replace the blockchain RPC or add another compliance model to a pool.

The adapter is implemented and tested with synthetic responses. A live accepted
shield transaction through it is **not yet verified**. Access and hosted service
availability must be checked with the operator; an ordinary explorer key is not
sufficient. No key or screening entitlement is included with gravity.

## Official reference map

Reviewed 2026-09-29. Follow the live contracts when these pages change:

| Contract | How gravity uses it |
|---|---|
| [Documentation index](https://starkscan.co/docs), [machine-readable index](https://starkscan.co/llms.txt) | Discover the maintained documentation; avoid inventing routes from names |
| [API discovery](https://starkscan.co/docs/api/discovery) | Inspect authenticated capabilities and caller scopes; use OpenAPI operation references for exact REST schemas |
| [Prover relay](https://starkscan.co/docs/api/strk20-prover) | Mainnet asynchronous jobs, idempotency, polling, one-delivery results and screening attestations |
| [Rate limits](https://starkscan.co/docs/api/rate-limits) | Honor service backoff; proving has a distinct daily/concurrency budget |
| [Retryable errors](https://starkscan.co/docs/api/retry) | Preserve unavailable/error states; never interpret them as an empty balance |
| [Starkscan RPC](https://starkscan.co/docs/rpc) | Blockchain JSON-RPC is a separate interface; it is not the hosted proving REST route |
| [Privacy Pool data API](https://starkscan.co/docs/api/privacy-pool) | Public indexed evidence is not private note ownership or the wallet's shielded balance |
| [Agent HTTP quickstart](https://starkscan.co/docs/api/agent-quickstart) | Bounded, authenticated integration and contract discovery |

The prover page documents `POST https://api.starkscan.co/v1/SN_MAIN/prove` and
`GET /v1/SN_MAIN/prove/{jobId}`. The adapter translates local
`starknet_proveTransaction` into that REST workflow. It is not a public Starknet
node. The docs also describe a dormant deployment returning 404: having `prove`
scope alone does not establish live availability.

## Start one shared adapter

Requirements: Node.js 24+, an operator-issued Starkscan key with `prove` scope,
and the other privacy services already configured. Store the key privately at
`~/.config/starkscan/api_key` (directory 0700, file 0600), or supply
`STARKSCAN_API_KEY` through a private process environment. Never put the value
in command arguments, source control, screenshots or agent prompts.

From the repository root:

```sh
node privacy/starkscan-relay.mjs
```

It binds **only** `http://127.0.0.1:3001`. Keep this single process running for
all clients using the key; do not start independent per-wallet counters. It
supports POST JSON-RPC at `/`, with `starknet_specVersion`, `starknet_chainId`,
`gravity_relayStatus`, and `starknet_proveTransaction`. The first three consume
no hosted proof allowance. A local spec response establishes adapter reachability
only, not Starkscan proving or screening availability.

```sh
curl --fail --silent --show-error http://127.0.0.1:3001/ \
  -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":1,"method":"gravity_relayStatus","params":[]}'
```

In **Privacy → Shielding prover**, click **Starkscan**. The choice saves
immediately. This setting is limited to the current default mainnet STRK20 pool. Keep
the general prover in Settings pointed at your existing self-hosted service.
The wallet never needs the Starkscan key. The adapter and hosted prover receive
the deposit's private inputs, including viewing material contained in the signed
virtual invocation. Use this route only when you trust those services.

For SDK clients, use the loopback adapter as the deposit `provingProvider.url`
with `chainId: constants.StarknetChainId.SN_MAIN`. Use a separate SDK instance
with the original proving provider for other operations. Keep discovery configured
separately. Allow up to 15 minutes for a proof; do not automatically rebuild or
resubmit after a timeout. Forward the full response to the SDK, including
`additional_data` as its `Proof.additionalData`.

## Shared budget and delivery

The adapter conservatively reserves at most ten new requests per key per UTC
day, including rejected or interrupted attempts. This is a **local limit**, not
Starkscan's advertised default or an authoritative remaining server allowance.
Requests made outside the adapter are invisible to its counter. Server refusals
and `Retry-After` remain authoritative.

Before sending, the adapter commits a request digest and idempotency key to a
private SQLite database. It never stores the private request itself. Identical
requests reuse that record across restarts. A lost submission response can be
recovered by resending the identical body; the adapter retains its original key.
Known pending jobs resume polling after startup without needing the input body.
The complete first terminal reply is committed before returning it to a client.

Storage is `~/.local/share/gravity/starkscan-relay/relay.sqlite` (0600 in a 0700
directory). It contains private proof results and upstream error diagnostics;
protect it and its backups. Status output excludes those payloads and the API key.
Do not delete this database to clear an unresolved request or reset a counter.

`unknown_delivery` and terminal `unavailable` stop new logical requests pending
operator reconciliation. Use the saved job ID and attempt count with support.
An interrupted response without a job ID requires the original request to recover
using the same idempotency key. The adapter never generates a replacement key
for an uncertain request. No public broadcast happens in this process.

The adapter requires a fresh screening signature on every returned deposit proof.
gravity checks its age again before spending (including after agent approval) and against the
chain timestamp before signing the public transaction. It reserves 60 seconds
for inclusion within the pool's current five-minute deadline. This cannot
guarantee inclusion; delayed transactions can still fail on chain.

## Validation boundary

Synthetic tests cover durable results, idempotent recovery, daily reset,
concurrent duplicates, access errors, backoff, uncertain delivery, expiry and
browser-origin rejection. Native wallet tests cover deposit-only routing and
expiry; UI tests cover shared-budget display and isolation from transfer actions.
Run `npm --prefix privacy test`, `npm --prefix desktop test`, and the Rust suite
in an environment that permits loopback listeners before deployment.

This remains unaudited wallet software. DYOR, inspect the code and service trust
model, and retain independent wallet/security review before production use.
