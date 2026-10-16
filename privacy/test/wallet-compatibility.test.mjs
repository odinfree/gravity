// Independent libraries reproduce the pre-upgrade Rust wallet snapshots.
// Public BIP-39 fixture only; this never reads the installed wallet or its keys.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {hkdfSync} from 'node:crypto';
import {mnemonicToSeedSync} from '@scure/bip39';
import {HDKey} from '@scure/bip32';
import {ec,hash,typedData} from 'starknet';
const read=name=>JSON.parse(readFileSync(new URL(`../../crates/wallet-core/tests/fixtures/${name}`,import.meta.url)));

test('legacy public keys, addresses and viewing keys reproduce independently',()=>{
 const fixture=read('legacy-wallet-v1.json');
 assert.equal(fixture.vectors.length,24);
 for(const v of fixture.vectors){
  const root=HDKey.fromMasterSeed(mnemonicToSeedSync(fixture.mnemonic,v.passphrase??''));
  const key=ec.starkCurve.grindKey(root.derive(v.path).privateKey);
  const pk=ec.starkCurve.getStarkKey(key);
  assert.equal(BigInt(pk),BigInt(v.public_key));
  assert.equal(BigInt(v.salt),BigInt(pk));
  assert.equal(BigInt(hash.calculateContractAddressFromHash(pk,v.class_hash,[pk],0)),BigInt(v.address));
  const privacyRoot=HDKey.fromMasterSeed(mnemonicToSeedSync(fixture.mnemonic,''));
  const branch=v.domain==='user'?0x5354524b:0x5354524c;
  const vkMaterial=ec.starkCurve.grindKey(privacyRoot.derive(`m/44'/9004'/${branch}'/0/${v.index}`).privateKey);
  const bytes=n=>Buffer.from(BigInt(n).toString(16).padStart(64,'0'),'hex');
  const info=Buffer.concat([Buffer.from(v.chain==='mainnet'?'SN_MAIN':'SN_SEPOLIA'),Buffer.from([0]),bytes(fixture.pool)]);
  const vk=Buffer.from(hkdfSync('sha256',bytes('0x'+vkMaterial),'strkd/strk20/viewing-key/v1',info,31));
  assert.equal('0x'+vk.toString('hex'),v.viewing_key);
 }
});

test('canonical SNIP-12 fixture remains independently reproducible',()=>{
 const fixture=read('snip12-canonical.json');
 assert.equal(fixture.vectors.length,12);
 for(const v of fixture.vectors)assert.equal(BigInt(typedData.getMessageHash(v.typed_data,v.account_address)),BigInt(v.message_hash));
});
