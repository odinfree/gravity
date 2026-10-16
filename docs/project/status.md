# gravity project status

Updated 2026-10-16. [Progress log](progress-log.md) · [Workflow](workflow.md).

## Resume here

Release the reviewed source changes, then obtain authorization for a disposable
Sepolia treasury test. The new `companion_treasury*` endpoint has passed source
and integration review. We have not installed, paired or live-tested that endpoint.
It rejects mainnet with `MAINNET_NOT_ENABLED`.

Account selection stays in Floodgate setup. The wallet requires approval of exact
recipients, amounts, nonce and fees. It persists reservations before approval and
signing state before submission. Accepted receipt/transaction/block evidence can
resolve an uncertain saved hash. Missing evidence keeps the account held; valid
conflicting requests receive bound unsigned cancellation. See the
[treasury contract](../code/treasury.md).

## Source and installed app

- The pending release includes the [October upstream backport](../code/upstream-fixes-2026-10.md),
  public treasury integration and documentation cleanup. It preserves addresses,
  viewing keys, v1 vaults and `org.starknet.strkd` identity.
- The prior installed macOS build contains the October backport. It predates the
  treasury endpoint. This release preparation does not change the installed app.
- Earlier live checks verified privacy registration and shielding. Private
  transfer and withdrawal still need accepted receipts and discovered balances.
  Prior checkpoint details remain in the progress log.

## Validation

- Baseline before documentation edits: Rust workspace **173 passed**, three opt-in
  checks skipped; strict clippy passed.
- Treasury review covers exact review/hash binding, grants, uint256 arithmetic,
  lock/revocation/expiry, concurrent requests, durable recovery and lost responses.
  Rust and JavaScript agree on the shared intent-hash fixture.
- Independent review fixed unsigned preflight recovery, abandoned reviews and the
  accepted-but-unknown account lock. Final targeted review found no blocker.
- Final release checks: Rust workspace **173 passed**; the two opt-in synthetic
  SDK/Rust pipeline tests also passed. The live prover-health check stayed skipped.
  Worker **43 passed** and desktop **21 passed**. Strict clippy, workspace build,
  worker bundle, TypeScript/Vite build and native desktop `cargo check` passed.
- The API catalog regression checks that supported treasury methods appear once
  and remain disjoint from the string-only deferred-method list.

## Blocked on review or live evidence

- Independent human review of crypto, viewing-key derivation, signing/pool actions
  and experimental `krusty-kms` before production mainnet use.
- Isolated portability and agent-isolation validation under the
  [portability test plan](../../spec/portability-test-plan.md).
- Authorized installation and disposable Sepolia treasury validation, including
  rejection, accepted success/revert and cross-application receipt accounting.
- Live private transfer and withdrawal acceptance with balance reconciliation.

## Remaining

- Standard `wallet_strk20*` API and additional token/pool or compliance adapters.
- Recovery for definitely rejected transactions that remain classified unknown;
  potentially signed uncertainty requires reconciliation, not a blind reset.
- Signed/notarized installers, bundled Node or another runtime, and platform
  validation beyond the current macOS build.
