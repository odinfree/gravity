use wallet_core::{privacy::viewing_key_v1, ChainId, Domain, Felt};
const SEED: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
#[test]
fn viewing_keys_are_seed_recoverable_and_domain_separated() {
    let pool = Felt::from(123u64);
    let key = viewing_key_v1(SEED, Domain::User, 0, ChainId::Mainnet, &pool).unwrap();
    assert_eq!(
        *key,
        *viewing_key_v1(SEED, Domain::User, 0, ChainId::Mainnet, &pool).unwrap()
    );
    for other in [
        viewing_key_v1(SEED, Domain::Agent, 0, ChainId::Mainnet, &pool),
        viewing_key_v1(SEED, Domain::User, 1, ChainId::Mainnet, &pool),
        viewing_key_v1(SEED, Domain::User, 0, ChainId::Sepolia, &pool),
        viewing_key_v1(SEED, Domain::User, 0, ChainId::Mainnet, &Felt::from(124u64)),
    ] {
        assert_ne!(*key, *other.unwrap());
    }
    assert_eq!(
        key.as_str(),
        "0x681c62069979335cbada2c211f64a63e4c1faea44e969f26469bf583b1adce"
    );
    assert_eq!(key.len(), 64); // 0x plus exactly 31 bytes: below MAX_VIEWING_KEY.
    assert_ne!(Felt::from_hex(&key).unwrap(), Felt::ZERO);
}
