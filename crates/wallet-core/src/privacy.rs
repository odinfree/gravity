//! Versioned, seed-recoverable STRK20 viewing key. Never a spending key.
//!
//! v1 uses separate hardened account branches, then HKDF-SHA256 binds the
//! viewing scalar to chain + pool. Pin this derivation forever once registered.
//! The 31-byte output is below half the Stark curve order. Audit required.
use crate::{ChainId, CoreError, Domain, Felt, Result};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

pub fn viewing_key_v1(
    mnemonic: &str,
    domain: Domain,
    index: u32,
    chain: ChainId,
    pool: &Felt,
) -> Result<Zeroizing<String>> {
    // Distinct from user account'=0 and agent account'=0x41.
    let branch = match domain {
        Domain::User => 0x5354_524b,
        Domain::Agent => 0x5354_524c,
    };
    let scalar = krusty_kms::derive_private_key_with_coin_type(mnemonic, index, branch, 9004, None)
        .map_err(|_| CoreError::Derivation)?;
    let material = Zeroizing::new(scalar.to_bytes_be());
    let mut info = Vec::new();
    info.extend_from_slice(match chain {
        ChainId::Mainnet => b"SN_MAIN",
        ChainId::Sepolia => b"SN_SEPOLIA",
    });
    info.push(0);
    info.extend_from_slice(&pool.to_bytes_be());
    let mut output = Zeroizing::new([0u8; 31]);
    Hkdf::<Sha256>::new(Some(b"strkd/strk20/viewing-key/v1"), material.as_ref())
        .expand(&info, output.as_mut())
        .map_err(|_| CoreError::Derivation)?;
    if output.iter().all(|b| *b == 0) {
        output[30] = 1;
    }
    Ok(Zeroizing::new(format!(
        "0x{}",
        hex::encode(output.as_ref())
    )))
}
