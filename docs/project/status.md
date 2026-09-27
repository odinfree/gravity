# gravity project status

Updated 2026-09-27. History: [progress log](progress-log.md).

## Resume here

The local gravity fork now has a native Privacy tab and authenticated privacy
agent methods. Build/run steps are in the [README](../../README.md); architecture,
configuration and recovery are in the [privacy guide](../code/privacy-stack.md).
The first pool adapter is STRK20 v2-compatible and currently handles STRK.
Different pool ABIs or compliance protocols require another reviewed adapter.

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
- Register, discover balances, prepare shield/transfer/unshield, review fees,
  final human approval, one-shot broadcast and receipt journal recovery.
- Wallet-owned SDK child, constrained Rust signing, exact allowances, explicit
  chain, mature reference blocks and private request-log redaction.

## Validation

- `cargo build`, `cargo test`: passed, 146 tests. Two opt-in checks skipped in
  the default suite (live prover health and the SDK pipeline).
- `cargo clippy --all-targets -- -D warnings`: passed.
- Explicit SDK/Rust pipeline check: passed against synthetic loopback services,
  covering real signing, review ownership, one-shot submission and receipt recovery.
- SDK worker: 14 tests passed, including all four action builders, screening
  policy, mature notes and wrong-chain rejection.
- Desktop: 12 React tests passed; TypeScript/Vite and unsigned Apple Silicon
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
- Operator-authorized screening integration for screened deposits. A plain
  self-hosted prover does not provide that authorization.
- Live accepted receipts and discovered balances for shield → transfer → withdraw
  through this integration. Registration is live-verified; shield/transfer/withdraw remain unverified.

## Remaining

- Standard `wallet_strk20*` API, other token/pool adapters and other compliance
  protocols, independent recovery/interoperability review.
- Automatic resolution of a definitely rejected transaction whose RPC submission
  was classified unknown; currently requires careful manual reconciliation.
- Signed/notarized installers, bundling Node or an equivalent runtime, and
  platform validation beyond the current macOS build.
