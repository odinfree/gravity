import './setup-dom';
import {test,beforeEach,afterEach} from 'node:test';
import assert from 'node:assert/strict';
import {act} from 'react';
import {createRoot,type Root} from 'react-dom/client';
import {Privacy} from '../src/components/Privacy';
import {parseStrk} from '../src/privacy-amount';
import {api,calls} from './privacy-mock-api';
let root:Root,host:HTMLDivElement;
const original={...api};
beforeEach(async()=>{Object.assign(api,original);calls.length=0;host=document.createElement('div');document.body.append(host);root=createRoot(host);await act(async()=>{root.render(<Privacy status={{network:'SN_MAIN'} as any}/>);});});
afterEach(async()=>{await act(async()=>root.unmount());host.remove();});
function button(s:string){const b=[...host.querySelectorAll('button')].find(b=>b.textContent===s);assert.ok(b,`button ${s}`);return b;}
async function click(s:string){await act(async()=>button(s).click());}
test('account status is automatic and one action opens the approval flow for exactly 10 STRK',async()=>{
  assert.ok(calls.some(c=>c.method==='status'));
  await click('Shield STRK');
  assert.equal(calls.find(c=>c.method==='prepare')!.value.amount,'10000000000000000000');
  assert.equal(calls.find(c=>c.method==='prepare')!.value.chain_id,'0x534e5f4d41494e');
  assert.equal(calls.filter(c=>c.method==='submit').length,1);
  assert.match(host.textContent!,/Transaction accepted/);
  assert.ok(![...host.querySelectorAll('button')].some(b=>b.textContent==='Done'));
  assert.ok(!button('Shield STRK').disabled,'accepted transaction returns directly to actions');
});
test('screening failures leave the wallet with no submission path',async()=>{
  api.privacyPrepare=async()=>{throw new Error('Screening signature required');};
  api.privacyStatus=async(v)=>({...await original.privacyStatus(v),deposit_screening:'signature_missing'});
  await click('Shield STRK');assert.match(host.querySelector('[role="alert"]')!.textContent!,/Screening/);
  assert.ok(button('Shielding unavailable').disabled);
  assert.ok(!calls.some(c=>c.method==='submit'));await click('Dismiss');
  assert.equal(host.querySelector('[role="alert"]'),null);
  assert.ok(button('Shielding unavailable').disabled,'dismissing an error must not claim shielding works');
  await act(async()=>{const select=host.querySelector('#privacy-operation') as HTMLSelectElement;select.value='transfer';select.dispatchEvent(new Event('change',{bubbles:true}));});
  assert.ok(!button('Private transfer').disabled,'screening applies only to deposits');
});
test('a confirmed receipt remains final when discovery fails',async()=>{
  api.privacyStatus=async(v)=>{if(v.mode==='balances')throw new Error('Discovery unavailable');return original.privacyStatus(v);};
  let receipts=0;api.privacyReceipt=async()=>{receipts++;return original.privacyReceipt();};
  await click('Shield STRK');
  assert.match(host.textContent!,/Transaction accepted/);
  assert.match(host.textContent!,/Shielded balance refresh is unavailable/);
  assert.ok(!button('Shield STRK').disabled);
  assert.equal(receipts,1);
});
test('unverified screening is never labelled ready',()=>{
  assert.match(host.textContent!,/Screening not verified/);
  assert.match(host.textContent!,/operator-issued access/);
});
test('rejecting the wallet approval never claims a transaction was accepted',async()=>{
  api.privacySubmit=async()=>{throw new Error('User refused');};
  await click('Shield STRK');assert.match(host.textContent!,/User refused/);
  assert.ok(!host.textContent?.includes('Transaction accepted'));assert.ok(!host.textContent?.includes('0xabc'));
});
test('switching account clears a transaction result and checks the new account',async()=>{
  await click('Shield STRK');
  await act(async()=>{const select=host.querySelector('#privacy-account') as HTMLSelectElement;select.value='0x456';select.dispatchEvent(new Event('change',{bubbles:true}));});
  assert.ok(!host.textContent?.includes('Transaction accepted'));
  assert.ok(calls.some(c=>c.method==='status'&&c.value.account==='0x456'));
});
test('service settings still have a Back path that never submits',async()=>{
  await click('Privacy services');await click('Back');assert.ok(!calls.some(c=>c.method==='submit'));
});
test('STRK input preserves base-unit precision and rejects exponent notation',()=>{
  assert.equal(parseStrk('10.000000000000000001'),'10000000000000000001');
  for(const value of ['0','-10','1e18','NaN','0.0000000000000000001'])assert.throws(()=>parseStrk(value));
});
test('hosted quota stops shielding while transfers remain available',async()=>{
  await act(async()=>{root.unmount();});
  api.privacySettings=async()=>{const value=await original.privacySettings();return {...value,settings:{...value.settings,mainnet:{...value.settings.mainnet,deposit_prover:'starkscan'}}};};
  api.privacyStatus=async(value)=>({...await original.privacyStatus(value),hosted_prover:{reachable:true,local_attempts:10,local_limit:10,local_remaining:0,retry_after_seconds:0,pending:[]}});
  root=createRoot(host);await act(async()=>{root.render(<Privacy status={{network:'SN_MAIN'} as any}/>);});
  assert.match(host.textContent!,/Hosted attempts today: 10 \/ 10 across local clients/);
  assert.ok(button('Shielding unavailable').disabled);
  await act(async()=>{const select=host.querySelector('#privacy-operation') as HTMLSelectElement;select.value='transfer';select.dispatchEvent(new Event('change',{bubbles:true}));});
  assert.ok(!button('Private transfer').disabled);
});
test('hosted selection is explicit and has no credential field',async()=>{
  await click('Privacy services');
  const select=host.querySelector('#privacy-deposit-prover') as HTMLSelectElement;
  assert.equal(select.value,'configured');
  await act(async()=>{select.value='starkscan';select.dispatchEvent(new Event('change',{bubbles:true}));});
  assert.match(host.textContent!,/Starkscan receives the deposit’s private proving inputs/);
  assert.equal(host.querySelector('input[type="password"]'),null);
  await click('Save privacy services');
  assert.equal(calls.find(c=>c.method==='settings')!.value.mainnet.deposit_prover,'starkscan');
  assert.ok(!calls.some(c=>c.method==='prepare'));
});
test('Back restores the saved prover choice rather than applying an unsaved route',async()=>{
  await click('Privacy services');
  const select=host.querySelector('#privacy-deposit-prover') as HTMLSelectElement;
  await act(async()=>{select.value='starkscan';select.dispatchEvent(new Event('change',{bubbles:true}));});
  await click('Back');
  assert.ok(!host.textContent?.includes('Starkscan adapter is not running'));
  assert.ok(!button('Shield STRK').disabled);
  assert.ok(!calls.some(c=>c.method==='settings'));
  await click('Privacy services');
  assert.equal((host.querySelector('#privacy-deposit-prover') as HTMLSelectElement).value,'configured');
});
