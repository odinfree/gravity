import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { amount, felt, endpoint, screeningRequired, approvalCalls, STRK } from '../worker.mjs';

test('amounts use exact integers and reject malformed or out-of-range values',()=>{
  assert.equal(amount('10000000000000000001'),10000000000000000001n);
  for(const value of ['0','-1','1e18','1.0',(1n<<256n).toString(),10]) assert.throws(()=>amount(value));
});
test('field normalization preserves decimal values and rejects field overflow',()=>{
  assert.equal(felt('16'),'0x10');assert.equal(felt('0x00010'),'0x10');
  assert.throws(()=>felt(((1n<<251n)+17n*(1n<<192n)+1n).toString()));
});
test('private endpoints cannot redirect secrets to plaintext public URLs or userinfo',()=>{
  for(const value of ['https://host.example/api','http://127.0.0.1:8080','http://[::1]:8080']) assert.doesNotThrow(()=>endpoint(value));
  for(const value of ['http://public.example','https://user:secret@host.example','https://host.example/#key','file:///tmp/x'])assert.throws(()=>endpoint(value));
});
test('deposit cannot proceed without the screening signature',()=>{
  assert.throws(()=>screeningRequired('deposit',{}),e=>e.code==='SCREENING_REQUIRED');
  assert.doesNotThrow(()=>screeningRequired('register',{}));
  assert.doesNotThrow(()=>screeningRequired('deposit',{additionalData:{signature:{sig_r:'0x1',sig_s:'0x2',issued_at:1}}}));
});
test('STRK deposit approval equals deposit plus pool fee, with no unlimited allowance',()=>{
  const calls=approvalCalls('0x123',6n*10n**18n,'deposit',STRK,10n*10n**18n);
  assert.equal(calls.length,1);assert.equal(calls[0].entrypoint,'approve');
  assert.deepEqual(calls[0].calldata,['0x123','0xde0b6b3a76400000','0x0']);
  const transfer=approvalCalls('0x123',6n*10n**18n,'transfer',STRK,10n*10n**18n);
  assert.equal(BigInt(transfer[0].calldata[1]),6n*10n**18n);
});
test('bundled worker emits a sanitized error and never echoes unknown secret input',()=>{
  const secret='synthetic-test-secret-not-a-real-key';
  const run=spawnSync(process.execPath,['../desktop/src-tauri/resources/privacy/worker.cjs'],{input:JSON.stringify({secret})+'\n',encoding:'utf8',timeout:10_000});
  assert.equal(run.status,1);assert.ok(!run.stdout.includes(secret));assert.equal(run.stderr,'');
  assert.equal(JSON.parse(run.stdout).kind,'error');
});
