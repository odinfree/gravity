# gravity

A local agent wallet for the **Starknet privacy stack**. Create accounts, pair an
agent, generate proofs with your own prover, and review transactions in a desktop
app. Signing keys stay in the Rust wallet core.

**STRK20 is the first pool adapter, not the wallet's identity.** This version
supports registration, shielded STRK balances, shielding, private transfers and
unshielding through STRK20 v2-compatible contracts. Pool addresses, discovery
services and deposit policy are configurable per network. Different contract
interfaces or compliance protocols require another adapter; they are not
implemented automatically by changing an address.

This is an independent fork of
[starknet-innovation/strkd](https://github.com/starknet-innovation/strkd), retaining
its wallet core, agent pairing, approval flow and proving infrastructure. Privacy
actions use the [Starknet privacy SDK](https://github.com/starkware-libs/starknet-privacy/tree/main/sdk).
It is a complete wallet repository, independent of any skills collection.

## What is implemented

- Create a new seed wallet, additional accounts and scoped agent accounts.
- Desktop **Privacy** tab: register, check balances, shield, transfer, unshield,
  review amount and fees, and reconcile transaction receipts.
- Your own Starknet transaction prover over HTTPS or a loopback SSH tunnel.
- A separate discovery service, with its address kept in local settings.
- Pool policy profiles: screening required, or contract-enforced policy for
  custom compatible pools. The known STRK20 deployments require screening.
- Authenticated agent methods for preparation, approval and receipt recovery.
- Exact token allowances, explicit networks, mature proof state, expiring reviews
  and no automatic broadcast retries after an uncertain response.

A prepared proof is not a completed payment. A successful accepted receipt and
refreshed note balances establish the result. The standard `wallet_strk20*`
methods are still deferred; this fork exposes `companion_privacy*` methods.

## Build and run

Requirements: Rust stable, Node.js **24+**, npm, and the platform dependencies
for [Tauri 2](https://v2.tauri.app/start/prerequisites/). The private SDK worker is
bundled as JavaScript; **Node itself is required on the machine running the app**.

```sh
git clone https://github.com/odinfree/gravity.git
cd gravity
npm --prefix privacy ci
npm --prefix desktop ci
cd desktop
npm run tauri build -- --config tauri.self-hosted.conf.json --bundles app --no-sign
```

On macOS, open `desktop/src-tauri/target/release/bundle/macos/gravity.app`.
This build uses your configured prover; it does not bundle a native prover.
For development, build the worker with `npm run build:privacy`, then run
`npm run tauri dev` from `desktop/`.

The SDK package is vendored from its public source so this build needs no GitHub
Packages credential. [Provenance and licenses](privacy/PROVENANCE.md).

## Set up privacy

1. Create/unlock your wallet and select the intended network.
2. In **Settings**, configure its blockchain RPC. Select `starknet-rpc` under
   Proving, enter your prover endpoint, save and restart.
3. Open **Privacy → Privacy services**. Select a compatible pool, its deposit
   policy and your discovery endpoint. Services must use the same chain/pool.
4. Fund and deploy the account. Privacy checks its pool registration automatically.
5. Click **Register**, **Shield STRK**, **Private transfer** or **Unshield**.
   gravity prepares the proof and opens one approval for the amount and fees.
6. Receipt checks and the registration/balance update run automatically.
   Success returns directly to the action form; there is no Done step.
   Dependent actions wait until accepted state is mature.

For screened pools, the prover must return the pool's screening attestation.
A bare self-hosted prover does **not** supply screening authorization; shielding
stops before submission if the required attestation is missing. Choosing a
custom policy cannot bypass an existing pool's contract rules.

[Complete privacy guide](docs/code/privacy-stack.md) ·
[Self-hosted prover setup](docs/code/self-hosted-prover.md) ·
[Documentation index](docs/index.md)

## Agents and CLI

Open **Connect** and copy the prompt for your agent. The local endpoint describes
its API at `GET /`; pair once, then use `companion_privacyStatus`,
`companion_privacyBalances`, `companion_privacyPrepare`, `companion_privacySubmit`,
`companion_privacyReceipt` and `companion_privacyHistory`.

Privacy spending always requires a concrete on-screen approval, including for
agents with generic auto-approval grants. Preparation never broadcasts.

```sh
cargo install --path crates/wallet-cli
gravity pair --name my-agent
gravity accounts
gravity usage
```

For upgrade compatibility, the app still uses the legacy `org.starknet.strkd`
identifier/data directory and `STRKD_*` configuration variables. Existing vaults,
accounts and pairings are preserved. Do not run upstream strkd and gravity
against that same data directory at the same time.

## Verification and limits

```sh
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
npm --prefix privacy run build
npm --prefix privacy test
npm --prefix desktop test
npm --prefix desktop run build
# Requires the built worker and Node 24+; entirely synthetic local services:
cargo test -p wallet-rpc --test privacy_pipeline -- --ignored
```

Automated tests cover the real SDK/stdio/signing pipeline using synthetic chain
and proof responses. They do not establish live pool acceptance. Live shielding,
private transfer and withdrawal acceptance for this new integration remain to be
verified with authorized test funds and the chosen pool's services.

**Experimental and unaudited. DYOR.** Inspect the code, pool and compliance model;
understand the fees and trust assumptions before using it. The new viewing-key
and signing paths need independent security review before production use. There
is no guarantee of privacy, compliance, recovery or financial safety. Provers and
discovery operators receive private/viewing material; use services you trust.

## Contributing to the documentation

Add new docs to [docs/index.md](docs/index.md). Update the current
[status](docs/project/status.md) and append a [progress entry](docs/project/progress-log.md)
with meaningful changes. Preserve upstream attribution and license notices.
Never commit a seed, private key, viewing key, RPC credential, wallet state or
agent bearer token.
