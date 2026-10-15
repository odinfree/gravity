//! Open an actual v1 ciphertext created by Gravity 4beb20a before the update.
//! Only the public BIP-39 test seed and public test passphrase are used here.
use serde_json::Value;
use wallet_core::{address_hex, typed_data_message_hash, ChainId, EncryptedVault, Felt, Registry};
use wallet_rpc::WalletSession;

#[test]
fn old_vault_unlocks_and_reseals_without_changing_accounts_or_ownership() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../wallet-core/tests/fixtures/legacy-wallet-v1.json"
    ))
    .unwrap();
    let vault: EncryptedVault = serde_json::from_value(fixture["vault"].clone()).unwrap();
    let expected: Registry = serde_json::from_value(fixture["registry"].clone()).unwrap();
    let pass = "public-test-vault-passphrase";
    assert_eq!(vault.version, 1);
    let mut session = WalletSession::new_locked(ChainId::Mainnet);
    assert!(session.unlock(&vault, "wrong-passphrase").is_err());
    assert!(session.is_locked());
    session.unlock(&vault, pass).unwrap();
    assert_eq!(session.registry().unwrap(), &expected);
    for account in &expected.accounts {
        let d = session.deployment_data_for(account).unwrap();
        assert_eq!(address_hex(&d.address), account.address);
        let canonical: Value = serde_json::from_str(include_str!(
            "../../wallet-core/tests/fixtures/snip12-canonical.json"
        ))
        .unwrap();
        let data = canonical["vectors"][0]["typed_data"].to_string();
        let hash = typed_data_message_hash(&data, &d.address).unwrap();
        let sig = session.sign_typed_data_for(account, &data).unwrap();
        let vector = fixture["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["address"] == account.address && v["passphrase"].is_null())
            .unwrap();
        let public_key = Felt::from_hex(vector["public_key"].as_str().unwrap()).unwrap();
        assert!(starknet_crypto::verify(&public_key, &hash, &sig.r, &sig.s).unwrap());
    }
    let resealed = session.reseal().unwrap();
    assert_eq!(resealed.version, 1);
    session.lock();
    session.unlock(&resealed, pass).unwrap();
    assert_eq!(session.registry().unwrap(), &expected);
    assert_eq!(
        session
            .registry()
            .unwrap()
            .scoped_for(Some("public-test-client"))
            .count(),
        3
    );
    assert_eq!(
        session
            .registry()
            .unwrap()
            .scoped_for(Some("another-client"))
            .count(),
        0
    );
}
