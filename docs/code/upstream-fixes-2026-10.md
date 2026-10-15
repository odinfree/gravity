# October upstream fixes and wallet compatibility

Gravity selectively adapts fixes from `starknet-innovation/strkd` through
`a10ec02` (2026-10-12). This is not a merge of upstream's new wallet model.

## Included fixes

| Upstream source | Gravity implementation |
|---|---|
| [4b3ee48 / #34](https://github.com/starknet-innovation/strkd/commit/4b3ee48219e470c3ed5484df5a22802d8f551202) | Pin `krusty-kms` at `2b6b7b5` for canonical SNIP-12 string and function-name selector hashing. Adapt the public-key API without changing derivation. |
| [21daa96](https://github.com/starknet-innovation/strkd/commit/21daa96a273384d6c0cf190a0306ef0784686978) | Compare SDK note/channel discovery at the mature proof base with a fresh, numbered chain head before signing/proving. Refuse a spent note, advanced channel/slot, or mismatched discovery block. |
| [1c7a3fe](https://github.com/starknet-innovation/strkd/commit/1c7a3fe1b67292c1ec46fa08c140fa506ff505a9) | Check public STRK for the deposit plus pool fee before a proof consumes resources. Other actions also need the public pool fee. Re-read the full u256 balance after estimating the padded network fee, before returning a review. |
| [4f1a8e3](https://github.com/starknet-innovation/strkd/commit/4f1a8e36b5f8bf0f8f94538e43ecb2567c49d0b6) | Explain the bundled PROOF1 trace-size limit and classify the CLI's final error rather than unrelated progress lines; strip ANSI formatting. |

Gravity already suppresses private upstream error text in its SDK/Rust pipeline
and excludes privacy payloads from request logs. Those protections are retained.
The proof-size diagnostic applies to the native SNIP-36 backend; the configured
self-hosted privacy route continues to withhold raw prover errors.

## Identity retained

- User key path: `m/44'/9004'/0'/0/i`.
- Agent key path: `m/44'/9004'/65'/0/j` (`0x41` branch).
- OpenZeppelin deployment salt: the account public key, with the same class hash.
- Vault version: **1**, with the existing serialization, registry and ownership.
- Privacy viewing key: Gravity's `viewing_key_v1`, including its chain/pool
  binding and legacy HKDF salt. Existing registrations keep the same key.
- App identifier/data directory: `org.starknet.strkd`; agent pairing and settings
  formats are unchanged.

Upstream's [breaking address/vault change](https://github.com/starknet-innovation/strkd/pull/24),
Bramble viewing-key derivation, Rust privacy replacement, relay route and sweep
are not imported. There is no wallet migration or automatic fund movement.
Matching Gravity's prior implementation does not establish cross-wallet portability.

## Typed-data input boundary

SNIP-12 `selector` values must be function names, such as `transfer`.
The Rust reference encoder and starknet.js disagree on pre-hashed hex selector
strings. Gravity refuses numeric or malformed names before signing, including
inside arrays, structs, enums and Merkle-tree leaves. It never rewrites the signed
document. The twelve independent vectors retain both accepted and rejected cases.
See [SNIP-12 selector encoding](https://github.com/starknet-io/SNIPs/blob/master/SNIPS/snip-12.md#when-x-is-a-selector).

The pinned encoder also rejects `i128` strings and non-negative JSON numbers;
only negative JSON numbers are currently supported for that type. Corrected
string hashes intentionally differ from signatures produced by the old encoder.

## Evidence and remaining limits

`crates/wallet-core/tests/fixtures/legacy-wallet-v1.json` was captured from
Gravity `4beb20a` with the original `b50afd6` dependency **before** updating it.
It contains only the public BIP-39 all-zero test vector and a public test vault
passphrase. Twenty-four cases pin user/agent paths, both networks, multiple
indices, BIP-39 passphrases, public keys, deployment data, viewing keys, invoke
hashes and signatures. A real v1 ciphertext from that capture is unlocked and
resealed by the updated wallet session, preserving account metadata and scope.
Do not regenerate these snapshots merely to make an upgrade pass.

The Node conformance tests independently reproduce the public keys, addresses
and privacy viewing keys with scure/starknet.js/Node HKDF. Rust signatures from
the reopened vault verify against the old public keys. Privacy tests use the real
SDK worker and synthetic node/discovery/prover services, checking that unsafe
preparation fails before signing/proving and never broadcasts.

State may still change while a proof is running. The preflight is not a reservation;
accepted receipts and balance reconciliation remain necessary. No real wallet,
credentials or funded transaction was used for these checks. Independent review
of the crypto dependency update and privacy guards remains required under
`docs/project/workflow.md` before production use.
