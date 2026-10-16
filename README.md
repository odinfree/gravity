# gravity

A Starknet desktop wallet with STRK20 privacy actions and an API for paired
applications and agents. You review spending in the app; the Rust wallet core
holds the signing keys.

Gravity is an independent fork of
[starknet-innovation/strkd](https://github.com/starknet-innovation/strkd). It retains
the upstream wallet core, pairing and proving infrastructure. Privacy actions use
the [Starknet privacy SDK](https://github.com/starkware-libs/starknet-privacy/tree/main/sdk).

## Current support

| Feature | Scope |
|---|---|
| Accounts | Seed wallets, additional user accounts and scoped agent accounts |
| Privacy | Registration, STRK shielding, private transfer and unshielding through STRK20 v2-compatible pools |
| Services | Per-network RPC, prover, discovery and deposit-policy settings |
| Screened deposits | Optional Starkscan adapter with shared quota accounting; operator-issued `prove` access required |
| Agent API | Authenticated preparation, native approval and receipt recovery through `companion_privacy*` |
| Public treasury | Exact STRK transfer review, mandatory approval and durable recovery through `companion_treasury*`; Sepolia only |

Registration and shielding have prior live receipts. Private transfer and
withdrawal still need live acceptance and balance reconciliation. The new treasury
endpoint has passed source tests and integration review; we have not installed or
live-tested it. See [current status](docs/project/status.md).

The wallet supports STRK20 v2-compatible contracts. Other tokens, pool interfaces
and compliance protocols need adapter work. The standard `wallet_strk20*` API
remains deferred.

## Build and run

Install Rust stable, Node.js **24+**, npm and the
[Tauri 2 platform dependencies](https://v2.tauri.app/start/prerequisites/).
The app bundles its JavaScript privacy worker and requires Node on the host.

```sh
git clone https://github.com/odinfree/gravity.git
cd gravity
npm --prefix privacy ci
npm --prefix desktop ci
cd desktop
npm run tauri build -- --config tauri.self-hosted.conf.json --bundles app --no-sign
```

On macOS, open `desktop/src-tauri/target/release/bundle/macos/gravity.app`.
This unsigned build uses your configured prover. For development, run
`npm run build:privacy`, then `npm run tauri dev` from `desktop/`.

The vendored SDK builds without a GitHub Packages credential.
[Provenance and licenses](privacy/PROVENANCE.md).

## Set up privacy

1. Create or unlock your wallet and select the network.
2. In **Settings**, configure the blockchain RPC. Select `starknet-rpc` under
   Proving, enter the prover endpoint, save and restart.
3. Open **Privacy → Privacy services**. Set the pool, deposit policy and discovery
   endpoint. Use the same chain and pool across services.
4. Fund and deploy the account. The Privacy screen checks pool registration.
5. Review the account, recipient, amount, pool fee and maximum network fee.
   Click **Register**, **Shield STRK**, **Private transfer** or **Unshield** once.
   That click authorizes proving and submission within the displayed fee ceilings;
   there is no second approval dialog. Excess fees stop submission.
6. Check the accepted receipt and refreshed registration or note balance before
   taking the next dependent action.

Known STRK20 pools require a screening attestation for deposits. A prover endpoint
alone does not provide that authorization. Obtain credentials from the operator
of the [screening sidecar](https://github.com/starkware-libs/starknet-privacy/blob/main/proof-interceptor/README.md),
or use the [Starkscan deposit adapter](docs/code/starkscan-screening.md) with
operator-issued `prove` access. The repository includes neither entitlement.
Choosing a custom policy cannot bypass the pool contract's rules.

Select the deposit adapter in the Privacy screen; other actions keep the
configured prover. Missing attestations produce **Shielding unavailable**.
A service health response does not verify screening access.

[Privacy guide](docs/code/privacy-stack.md) ·
[Prover setup](docs/code/self-hosted-prover.md) ·
[Documentation index](docs/index.md)

## Public treasury transfers

External applications such as Floodgate can request public STRK transfers from a
selected user account. You approve exact recipients, uint256 amounts, nonce,
resource bounds, fee cap and expiry. The wallet binds the request to an immutable
operation and plan hash, including for callers with auto-approval grants.

The endpoint submits once and returns status and a transaction hash without
exporting signatures. Recover the original operation after an uncertain response.
Accepted receipt, transaction and block evidence can resolve a saved uncertain
hash; missing or inconsistent evidence keeps the reservation held.

**The treasury endpoint rejects mainnet with `MAINNET_NOT_ENABLED`.** Independent
human review and an authorized Sepolia validation remain required. Read the
[request schema and recovery rules](docs/code/treasury.md) before integration.

## Agents and CLI

Open **Connect** and copy the pairing prompt. The loopback service documents its
API at `GET /`. Privacy methods include `companion_privacyStatus`,
`companion_privacyBalances`, `companion_privacyPrepare`, `companion_privacySubmit`,
`companion_privacyReceipt` and `companion_privacyHistory`.

Agent privacy spending requires native approval, including for agents with
auto-approval grants. Preparation does not broadcast. The desktop's one-click
privacy action is a separate authorization path with explicit fee ceilings.

```sh
cargo install --path crates/wallet-cli
gravity pair --name my-agent
gravity accounts
gravity usage
```

Gravity preserves the legacy `org.starknet.strkd` app identifier and data directory,
`STRKD_*` configuration variables, account addresses, viewing keys, pairings and
v1 vault format. Do not run upstream strkd and Gravity against the same data
directory at the same time. The [October backport](docs/code/upstream-fixes-2026-10.md)
includes signing and preflight fixes; it excludes upstream's breaking address
and vault migration.

## Verification and security limits

```sh
cargo build --locked --workspace
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
npm --prefix privacy run build
npm --prefix privacy test
npm --prefix desktop test
npm --prefix desktop run build
# Synthetic SDK/Rust pipeline; requires the built worker and Node 24+:
STRKD_PRIVACY_NODE="$(command -v node)" cargo test --locked -p wallet-rpc --test privacy_pipeline -- --ignored
```

Tests use public test keys and synthetic services. Verify live acceptance with
accepted receipts and refreshed balances before claiming a working funded flow.

Independent human review of the crypto, viewing-key and signing paths remains a
gate for production mainnet use. Prover and discovery operators receive private
or viewing material; choose them with that access in mind. Automated checks do
not establish privacy, account portability or recovery guarantees.

## Contributing to the documentation

Link new docs from [docs/index.md](docs/index.md). Update
[status](docs/project/status.md) and prepend a [progress entry](docs/project/progress-log.md)
without changing older entries. Preserve upstream attribution and license notices.
Keep seeds, private and viewing keys, RPC credentials, wallet state and bearer
tokens out of commits.
