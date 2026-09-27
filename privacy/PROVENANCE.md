# Privacy SDK provenance

- Source: https://github.com/starkware-libs/starknet-privacy/tree/b40bf109c8e5b97d9d6e8ec6db48b16b9d10a106/sdk
- Package: `@starkware-libs/starknet-privacy-sdk` `0.14.3-rc.8`.
- Archive: `vendor/starkware-libs-starknet-privacy-sdk-0.14.3-rc.8.tgz`.
- SHA-256: `d6eb527b8eb8f07c88245844921c245b97f3f4447b57546571bb173293fc09e0`.
- Built from the unmodified public SDK source at the pinned commit; no registry
  token is needed to install it. Archive integrity is also pinned in the lockfile.
- The package metadata declares `ISC`; the source repository ships Apache-2.0.
  Both metadata and the repository's unmodified [license](vendor/LICENSE.starknet-privacy)
  are retained. gravity does not replace upstream copyright or licensing terms.

`build.mjs` bundles only the runtime import graph and retains legal comments.
The SDK lists development/devnet dependencies as production dependencies; npm
reports vulnerabilities in that dependency tree. The archive-extraction packages
`starknet-devnet` and `decompress` are not used by this wallet, and a build-time
metafile check rejects them if they enter the shipped worker. This is a limited
mitigation, not a claim that npm audit or a security audit is clean. Recheck
upstream fixes before updating the pin. The standard dependency audit should
still be run and reviewed by operators.

The SDK is trusted wallet code: it handles viewing material. Signing keys remain
inside Rust. See [privacy architecture](../docs/code/privacy-stack.md).
