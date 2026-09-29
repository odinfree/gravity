# gravity project status

Updated 2026-09-29. History: [progress log](progress-log.md).

## Resume here

The local gravity fork now has a native Privacy tab and authenticated privacy
agent methods. Build/run steps are in the [README](../../README.md); architecture,
configuration and recovery are in the [privacy guide](../code/privacy-stack.md).
The first pool adapter is STRK20 v2-compatible and currently handles STRK.
Different pool ABIs or compliance protocols require another reviewed adapter.
The [Starkscan deposit adapter](../code/starkscan-screening.md) is running locally.
Operator `prove` access and a live screened deposit are verified: three 10 STRK
mainnet deposits have successful receipts, and discovery increased by 30 STRK.
Next: finish private transfer and withdrawal with accepted receipts and balance
reconciliation. The native one-click change passes synthetic tests and is being
installed for live verification. Do not claim the complete funded round trip yet.

## Implemented

- gravity branding, standalone repository configuration, CLI and build workflow;
  existing app identifier, vault and key derivation compatibility retained.
- Account creation/import with Back navigation, agent pairing and existing public
  wallet methods; network selection now persists across restart.
- Selected self-hosted `starknet-rpc` prover; no forced hosted service or native
  prover in the self-hosted app bundle.
- Pool/discovery/deposit-policy settings per network. Known pools require
  screening; custom compatible deployments defer policy enforcement to their
  contract when explicitly configured.
- Native one-click register/shield/transfer/unshield within displayed pool and
  network fee ceilings. Agent preparation/submission retains explicit approval.
  One-shot broadcast and receipt journal recovery are shared by both paths.
- Visible account/action/prover choices replace native dropdowns; selecting a
  shielding prover saves immediately without changing the other prover route.
- Wallet-owned SDK child, constrained Rust signing, exact allowances, explicit
  chain, mature reference blocks and private request-log redaction.
- Optional mainnet deposit-only Starkscan routing, shared SQLite accounting and
  idempotent recovery, private result persistence, and attestation expiry checks.

## Validation — current change

- Nine shared-relay tests passed with injected HTTP responses, including UTC
  limits, interrupted delivery, result persistence, error redaction and expiry.
- Five targeted Rust privacy tests passed. `cargo build` and strict workspace
  clippy passed. Seventeen desktop tests and the TypeScript/Vite build passed.
  The unsigned Apple Silicon app bundle builds; the hosted adapter build is installed.
- Full Rust workspace: 149 tests passed. The separate SDK/Rust signing pipeline
  also passed against synthetic loopback services. Two opt-in checks are excluded
  from the default run; the synthetic pipeline was run explicitly afterward.
- All 25 SDK/relay tests passed, including socket-based action-builder tests.
  The earlier sandbox `EPERM` limitation is resolved for these checks.
- Live hosted proving returned screening signatures. Three approved 10 STRK
  deposits succeeded and discovery reconciled the 30 STRK increase. A fourth
  proof was prepared but its final spending confirmation was rejected.
- Native one-click tests pass with exact fee ceilings; excessive pool/network
  fees produce zero broadcasts and the existing submit API still rejects
  denied approvals. Both synthetic SDK/Rust pipelines pass.
- The full workspace tests, strict clippy, desktop tests and frontend build pass.
  Native one-click live execution is pending.

## Previous validation — 2026-09-27

- `cargo build`, `cargo test`: passed, 147 tests. Two opt-in checks skipped in
  the default suite (live prover health and the SDK pipeline).
- `cargo clippy --all-targets -- -D warnings`: passed.
- Explicit SDK/Rust pipeline check: passed against synthetic loopback services,
  covering real signing, review ownership, one-shot submission and receipt recovery.
- SDK worker: 16 tests passed, including all four action builders, screening
  policy, mature notes and wrong-chain rejection.
- Desktop: 14 React tests passed; TypeScript/Vite and unsigned Apple Silicon
  `gravity.app` build passed. Native app launches using the existing vault; Privacy shows live registration
  and fees. A real pool-registration proof succeeded on the configured prover,
  and its final public call obtained a mainnet fee estimate. A user-approved
  registration then succeeded with an accepted on-chain receipt and matching key.
- SDK dependency tree still has audit findings in upstream development tooling;
  excluded from the shipped worker by a checked build graph. See provenance.

## Blocked (needs review / external services)

- Independent security review of the new viewing-key derivation and signing/
  pool-action path, plus inherited experimental `krusty-kms`, before production
  mainnet use. Structural tests do not establish audit completion or portability.
- Live accepted receipts and discovered balances for shield → transfer → withdraw
  through this integration. Registration and shielding are live-verified;
  transfer/withdrawal remain unverified.

## Remaining

- Standard `wallet_strk20*` API, other token/pool adapters and other compliance
  protocols, independent recovery/interoperability review.
- Automatic resolution of a definitely rejected transaction whose RPC submission
  was classified unknown; currently requires careful manual reconciliation.
- Signed/notarized installers, bundling Node or an equivalent runtime, and
  platform validation beyond the current macOS build.
