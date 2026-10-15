# gravity project status

Updated 2026-10-15. History: [progress log](progress-log.md).

## Resume here

Selected upstream fixes are implemented locally on
`codex/upstream-compatible-fixes`: canonical typed-data signing, stale private
state checks, public fee/funding checks and native proof-size diagnostics.
Existing Gravity addresses, viewing keys, v1 vaults, app identifier and account
ownership are preserved and regression-tested. See the
[compatibility record](../code/upstream-fixes-2026-10.md).

The unsigned self-hosted macOS app builds in this checkout. It has not replaced
the installed app, and this update has not been published or used for a funded
transaction. Next: independent review of the crypto dependency and privacy
guards, followed by an operator-controlled rollout. Current services/balances
were not inspected; the following live checkpoint is historical.

### Prior live checkpoint — 2026-09-29

The local gravity fork now has a native Privacy tab and authenticated privacy
agent methods. Build/run steps are in the [README](../../README.md); architecture,
configuration and recovery are in the [privacy guide](../code/privacy-stack.md).
The first pool adapter is STRK20 v2-compatible and currently handles STRK.
Different pool ABIs or compliance protocols require another reviewed adapter.
The [Starkscan deposit adapter](../code/starkscan-screening.md) is running locally.
Operator `prove` access and a live screened deposit are verified: three 10 STRK
mainnet deposits have successful receipts, and discovery increased by 30 STRK.
At that checkpoint, the next step was private transfer and withdrawal with accepted receipts and balance
reconciliation. The native one-click change is installed, and its code commit passed CI.
The recipient account has since registered and received a successful shield
through the one-click build. Progress feedback and percentage amount controls
are implemented and validated; transfer/withdrawal reconciliation remains next. Do not claim the complete funded round trip yet.

## Implemented

- Selective October upstream fixes while retaining all Gravity wallet identity
  formats; old vault, public-key/address, privacy-key and signature fixtures pass.
- Privacy preparation refuses stale notes/channels and insufficient public funds
  before proving; final review also requires the estimated maximum network fee.
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
- Always-visible transaction stages and elapsed time, driven by backend events
  and receipts. Unshield/transfer show spendable funds beside 25/50/100 percent
  shortcuts; 100 percent selects 95 percent and preserves a shielded buffer.
- Wallet-owned SDK child, constrained Rust signing, exact allowances, explicit
  chain, mature reference blocks and private request-log redaction.
- Optional mainnet deposit-only Starkscan routing, shared SQLite accounting and
  idempotent recovery, private result persistence, and attestation expiry checks.

## Validation — current change

- `cargo test --locked --workspace`: 154 passed; the two opt-in synthetic SDK/Rust
  pipeline tests also passed explicitly. The live prover-health check was skipped.
- Strict workspace clippy and build passed. The 24 pre-update identity vectors
  pass and are independently reproduced with scure/starknet.js/Node HKDF.
- A v1 vault captured before the dependency update unlocks/reseals, retains
  accounts/ownership, and signs messages verifiable with the old public keys.
- Twelve typed-data reference cases cover strings, function-name selectors,
  nested arrays, enums and Merkle trees, with ambiguous numeric selectors refused.
- Privacy/relay/conformance suite: 37 passed. Desktop: 21 passed; TypeScript/Vite
  and unsigned self-hosted Apple Silicon app builds passed.
- All new transaction tests use synthetic services and public test keys. No
  claim of live acceptance, independent audit or installation of this update.

## Previous validation — 2026-09-29

- Nine shared-relay tests passed with injected HTTP responses, including UTC
  limits, interrupted delivery, result persistence, error redaction and expiry.
- Five targeted Rust privacy tests passed. `cargo build` and strict workspace
  clippy passed. Twenty-one desktop tests and the TypeScript/Vite build passed.
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
  Native one-click registration and shielding are now live-verified.
- Progress stages and fee checks pass both SDK/Rust pipelines. Percentage
  controls use exact base units, exclude unsettled funds, and disable on missing
  discovery; the wallet-size UI preview was visually checked with synthetic data.

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

- Independent review of the October `krusty-kms` update, selector input guard,
  and privacy state/funds checks before production rollout. Regression tests
  establish local compatibility, not an independent security audit.
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
