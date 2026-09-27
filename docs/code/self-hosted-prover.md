# Use your own Starknet transaction prover

Select the `starknet-rpc` backend to send proving requests to an existing
Starknet transaction-prover service. The wallet keeps its existing signing and
approval flow. The service receives the signed virtual transaction and returns
the proof; this backend starts no local prover process and broadcasts nothing.

This is a transport adapter for `companion_prove` and compatible
`companion_signAndProve` requests. It does not implement the deferred
`wallet_strk20*` methods, pool-action construction, discovery, or viewing-key
management. Those remain Phase 3 work.

## Configure

1. Keep the service bound to loopback on its host. If it runs on another machine,
   establish an SSH tunnel from the machine running `strkd`, for example:

   ```sh
   ssh -N -o ExitOnForwardFailure=yes -o ServerAliveInterval=30 \
     -L 127.0.0.1:3000:127.0.0.1:3000 YOUR_USER@YOUR_HOST
   ```

2. In **Settings → Proving**, select **starknet-rpc — use your Starknet
   transaction prover**. Set the corresponding network's prover URL to
   `http://127.0.0.1:3000`. Use the full JSON-RPC endpoint; do not add `/v1/prove`.
3. Set the normal blockchain RPC for that same network in the wallet's RPC
   Settings. This remains separate from the prover endpoint and is mirrored
   into the prover settings by the desktop app.
4. Save and restart the app. A persisted backend selection takes precedence
   over `STRKD_PROVER`; `STRKD_PROVER=starknet-rpc` is also available when no
   backend has been saved. This backend never falls back to the bundled prover.

Put a mainnet prover in the **Mainnet** field, not the Sepolia field. For a
Sepolia test wallet, run a separate Sepolia-configured prover. The adapter
verifies the blockchain RPC chain ID and the prover's RPC v0.10 interface.
The proving API does not expose its chain ID, so its chain configuration must
also be verified by the operator. A health response alone cannot establish it.

Loopback HTTP and HTTPS endpoints are accepted. Public plaintext HTTP,
userinfo, fragments, redirects and environment proxies are rejected/disabled.
The legacy `prover_api_key` setting is not sent by this backend. Use the SSH
tunnel for a private self-hosted service; no Alchemy key belongs in the prover
URL. The prover's own blockchain RPC credential stays in its service config.

## Build without the bundled native prover

The optional Tauri config leaves native prover resources out of the app:

```sh
cd desktop
npm ci
npm run tauri build -- --config tauri.self-hosted.conf.json --bundles app
```

No `stage-prover*.sh` step is needed. Select `starknet-rpc` in Settings after
launch; the resource override itself does not change the selected backend.

## Request and response contract

The existing `companion_prove` request carries:

```json
{
  "network": "testnet",
  "payload": {
    "block_number": 12345,
    "transaction": "REPLACE_WITH_THE_COMPLETE_SIGNED_INVOKE_V3_OBJECT"
  }
}
```

The string above is an explanatory placeholder, not an executable transaction.
The adapter sends `starknet_proveTransaction` with named params
`{block_id: {block_number}, transaction}`. It preserves every supplied
transaction field, including its signature, and returns the service's
`{proof, proof_facts, l2_to_l1_messages, ...}` result, including optional
`additional_data`.

The virtual invoke must already have a signature, zero tip and zero effective
fee for each resource (`max_amount × max_price_per_unit = 0`). Sign those exact
bounds initially; changing bounds after signing invalidates the signature.
Execution gas amounts can still be nonzero when their price is zero. These
virtual bounds are distinct from the gas bounds for a later onchain submission.

If `block_number` is omitted, the adapter uses node head minus ten. The caller
must sign with the nonce and state appropriate to that block. For dependent
operations, choose the block explicitly after the prior state is mature.
The adapter never sends virtual calldata to the blockchain node for simulation
or fee estimation.

Proving has a 15-minute request timeout; metadata checks have a 15-second
timeout. Responses are limited to 32 MiB. Busy and other failures terminate
the job with a sanitized error; there is no automatic retry or broadcast.
Reconcile the active job before requesting another expensive proof.

## Privacy and validation boundaries

The prover operator sees the virtual transaction's private inputs. For STRK20
those can include viewing material. Use only a prover/host you trust. Signing
keys are not passed by this adapter.

For this backend, persisted proof records omit the original request payload.
Wallet request logs omit proving request/result bodies and raw error text even
with full-payload debugging enabled. Proofs, job metadata and labels remain
stored; keep labels free of secrets. Existing historical logs are not rewritten.

Synthetic tests exercise the real HTTP adapter, network mismatch rejection,
unchanged signatures, private-input routing, errors, redirects, settings
selection and payload omission. The optional read-only check uses an already
running service and requests only `starknet_specVersion`:

```sh
cargo test -p prover --test starknet_rpc
cargo test -p prover --test starknet_rpc live_loopback_prover_health -- --ignored --nocapture
```

These checks do not prove an end-to-end private transfer. Wallet/mainnet use
remains subject to the repository's [security gates](../project/workflow.md#security-gates-non-negotiable).

Protocol references: [official prover API](https://github.com/starkware-libs/sequencer/blob/avi/privacy/configmap-docs/crates/starknet_transaction_prover/src/server/rpc_api.rs)
and [Starknet Privacy compatibility matrix](https://github.com/starkware-libs/starknet-privacy#compatibility-matrix).
