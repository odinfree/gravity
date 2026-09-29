import './setup-dom';
import {test,beforeEach,afterEach} from 'node:test';
import assert from 'node:assert/strict';
import {act} from 'react';
import {createRoot,type Root} from 'react-dom/client';
import {Privacy} from '../src/components/Privacy';
import {parseStrk,percentStrk} from '../src/privacy-amount';
import {api,calls} from './privacy-mock-api';
let root:Root,host:HTMLDivElement;
const original={...api};
beforeEach(async()=>{Object.assign(api,original);calls.length=0;host=document.createElement('div');document.body.append(host);root=createRoot(host);await act(async()=>{root.render(<Privacy status={{network:'SN_MAIN'} as any}/>);});});
afterEach(async()=>{await act(async()=>root.unmount());host.remove();});
function button(s:string){const b=[...host.querySelectorAll('button')].find(b=>b.textContent===s);assert.ok(b,`button ${s}`);return b;}
async function click(s:string){await act(async()=>button(s).click());}
test('account status is automatic and one action submits exactly 10 STRK with the displayed fee ceilings',async()=>{
  assert.ok(calls.some(c=>c.method==='status'));
  await click('Shield STRK');
  assert.equal(calls.find(c=>c.method==='execute')!.value.amount,'10000000000000000000');
  assert.equal(calls.find(c=>c.method==='execute')!.value.chain_id,'0x534e5f4d41494e');
  assert.equal(calls.filter(c=>c.method==='execute').length,1);
  assert.deepEqual(calls.find(c=>c.method==='execute')!.value.limits,{max_pool_fee:'6000000000000000000',max_network_fee:'5000000000000000000'});
  assert.ok(!calls.some(c=>c.method==='prepare'||c.method==='submit'),'no second review command');
  assert.match(host.textContent!,/Transaction accepted/);
  assert.ok(![...host.querySelectorAll('button')].some(b=>b.textContent==='Done'));
  assert.ok(!button('Shield STRK').disabled,'accepted transaction returns directly to actions');
});
test('screening failures leave the wallet with no submission path',async()=>{
  api.privacyExecute=async()=>{throw new Error('Screening signature required');};
  api.privacyStatus=async(v)=>({...await original.privacyStatus(v),deposit_screening:'signature_missing'});
  await click('Shield STRK');assert.match(host.querySelector('[role="alert"]')!.textContent!,/Screening/);
  assert.ok(button('Shielding unavailable').disabled);
  assert.ok(!calls.some(c=>c.method==='submit'));await click('Dismiss');
  assert.equal(host.querySelector('[role="alert"]'),null);
  assert.ok(button('Shielding unavailable').disabled,'dismissing an error must not claim shielding works');
  await act(async()=>{(host.querySelector('input[name="privacy-operation"][value="transfer"]') as HTMLInputElement).click();});
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
test('a failed one-click transaction never claims acceptance',async()=>{
  api.privacyExecute=async()=>{throw new Error('Prepared fees exceed the displayed limits');};
  await click('Shield STRK');assert.match(host.textContent!,/fees exceed/);
  assert.ok(!host.textContent?.includes('Transaction accepted'));assert.ok(!host.textContent?.includes('0xabc'));
});
test('switching account clears a transaction result and checks the new account',async()=>{
  await click('Shield STRK');
  await act(async()=>{(host.querySelector('input[name="privacy-account"][value="0x456"]') as HTMLInputElement).click();});
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
  await act(async()=>{(host.querySelector('input[name="privacy-operation"][value="transfer"]') as HTMLInputElement).click();});
  assert.ok(!button('Private transfer').disabled);
});
test('one click saves the hosted choice without preparing or submitting a transaction',async()=>{
  const choice=host.querySelector('input[name="privacy-deposit-prover"][value="starkscan"]') as HTMLInputElement;
  assert.equal(choice.checked,false);
  assert.ok((host.querySelector('input[name="privacy-deposit-prover"][value="configured"]') as HTMLInputElement).checked);
  await act(async()=>{choice.click();});
  assert.equal(choice.checked,true);
  assert.match(host.textContent!,/Starkscan receives the deposit’s private proving inputs/);
  assert.equal(host.querySelector('input[type="password"]'),null);
  assert.equal(calls.filter(c=>c.method==='settings').length,1);
  assert.equal(calls.find(c=>c.method==='settings')!.value.mainnet.deposit_prover,'starkscan');
  assert.ok(!calls.some(c=>c.method==='submit'));
  assert.match(host.textContent!,/both services available/);
  assert.ok(!calls.some(c=>c.method==='execute'));
});
test('a failed prover save retains the previous selection and never submits',async()=>{
  api.setPrivacySettings=async()=>{throw new Error('Could not save services');};
  await act(async()=>{(host.querySelector('input[name="privacy-deposit-prover"][value="starkscan"]') as HTMLInputElement).click();});
  assert.equal((host.querySelector('input[name="privacy-deposit-prover"][value="configured"]') as HTMLInputElement).checked,true);
  assert.match(host.querySelector('[role="alert"]')!.textContent!,/Could not save/);
  assert.ok(!calls.some(c=>c.method==='submit'||c.method==='execute'));
});

test('progress is immediate, follows real stages, and waits for a successful receipt',async()=>{
  let stage!: (value:"preparing"|"proving"|"checking_fees"|"submitting")=>void;
  let finish!: (value:any)=>void;
  let confirm!: (value:any)=>void;
  api.privacyExecute=(_request,_limits,onProgress)=>{stage=onProgress!;return new Promise(resolve=>{finish=resolve;});};
  api.privacyReceipt=()=>new Promise(resolve=>{confirm=resolve;});
  await click('Shield STRK');
  assert.match(host.querySelector('[aria-label="Transaction progress"]')!.textContent!,/Preparing transaction/);
  assert.ok(button('Preparing…').disabled);
  await act(async()=>stage('proving'));
  assert.match(host.querySelector('[role="progressbar"]')!.getAttribute('aria-valuetext')!,/Generating proof/);
  assert.equal(host.querySelector('[role="progressbar"]')!.getAttribute('aria-valuenow'),null,'no invented percentage');
  await act(async()=>stage('checking_fees'));
  assert.match(host.textContent!,/Checking proof and fees/);
  await act(async()=>stage('submitting'));
  assert.match(host.textContent!,/Submitting transaction/);
  await act(async()=>finish({transaction_hash:'0xabc',status:'submission_unknown',chain_id:'0x534e5f4d41494e'}));
  assert.match(host.querySelector('[aria-label="Transaction progress"]')!.textContent!,/Waiting for confirmation/);
  assert.ok(!host.textContent!.includes('Transaction confirmed'));
  await act(async()=>stage('proving')); // Late channel message must not regress confirmation.
  assert.match(host.querySelector('[aria-label="Transaction progress"]')!.textContent!,/Waiting for confirmation/);
  await act(async()=>confirm(await original.privacyReceipt()));
  assert.match(host.querySelector('[aria-label="Transaction progress"]')!.textContent!,/Transaction confirmed/);
});
test('unshield discovers and displays funds; percentage buttons use spendable notes and preserve 5 percent',async()=>{
  await act(async()=>{(host.querySelector('input[name="privacy-operation"][value="withdraw"]') as HTMLInputElement).click();});
  assert.ok(calls.some(c=>c.method==='status'&&c.value.mode==='balances'));
  assert.match(host.querySelector('[aria-label="Available shielded funds"]')!.textContent!,/Available to unshield80 STRK/);
  const amount=host.querySelector('#privacy-amount') as HTMLInputElement;
  await click('25%');assert.equal(amount.value,'20');
  await click('50%');assert.equal(amount.value,'40');
  await click('100%');assert.equal(amount.value,'76');
  assert.ok(!calls.some(c=>c.method==='execute'),'choosing an amount never spends');
  await click('Unshield');
  assert.equal(calls.find(c=>c.method==='execute')!.value.amount,'76000000000000000000');
});
test('percentage arithmetic floors base units without rounding up or losing precision',()=>{
  assert.equal(percentStrk('1000000000000000001',100),'0.95');
  assert.equal(percentStrk('99999999999999999999999',25),'24999.999999999999999999');
  assert.equal(percentStrk('0',100),'0');
});
test('unavailable shielded discovery disables percentage shortcuts instead of inventing a balance',async()=>{
  api.privacyStatus=async(value)=>{if(value.mode==='balances')throw new Error('Discovery unavailable');return original.privacyStatus(value);};
  await act(async()=>{(host.querySelector('input[name="privacy-operation"][value="withdraw"]') as HTMLInputElement).click();});
  assert.ok(button('25%').disabled);assert.ok(button('50%').disabled);assert.ok(button('100%').disabled);
  assert.ok(button('Unshield').disabled);
  assert.match(host.textContent!,/Shielded balance unavailable/);
});
