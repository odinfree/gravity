import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { StarkscanRelay, freshScreening, allowedRequest, validateInput } from '../starkscan-relay.mjs';

const input = (block = 100) => ({ block_id: { block_number: block }, transaction: {
  type: 'INVOKE', version: '0x3', sender_address: '0x123', nonce: '0x0', tip: '0x0',
  calldata: ['0xdeadbeef0123456789'], signature: ['0x1', '0x2'], paymaster_data: [], account_deployment_data: [],
  resource_bounds: Object.fromEntries(['l1_gas', 'l2_gas', 'l1_data_gas'].map(k => [k, { max_amount: '0x0', max_price_per_unit: '0x0' }]))
} });
const json = (body, status = 200, headers = {}) => new Response(JSON.stringify(body), { status, headers });
const proof = now => ({ proof: 'synthetic-proof', proof_facts: ['0x1'], l2_to_l1_messages: [],
  additional_data: { signature: { issued_at: Math.floor(now / 1000), sig_r: '0x12', sig_s: '0x34' } } });
const queued = { jobId: 'prv_12345678', status: 'queued', terminal: false, pollAfterSeconds: 10 };
function fixture(t, fetchImpl) {
  const directory = mkdtempSync(join(tmpdir(), 'gravity-relay-test-'));
  let now = Date.UTC(2026, 8, 29, 12); const calls = [], waits = [];
  const opts = { apiKey: 'synthetic-test-key', directory, now: () => now,
    wait: async ms => { waits.push(ms); now += ms; },
    fetchImpl: async (url, init) => {
      calls.push({ url, init });
      return fetchImpl ? fetchImpl(url, init, () => now, calls) :
        init.method === 'POST' ? json(queued, 202) : json({ ...queued, status: 'succeeded', terminal: true, result: proof(now) });
    } };
  let relay = new StarkscanRelay(opts);
  t.after(() => { relay.close(); rmSync(directory, { recursive: true }); });
  return { get relay() { return relay; }, calls, waits, directory, now: () => now,
    setNow: v => { now = v; }, restart: () => { relay.close(); relay = new StarkscanRelay(opts); return relay; } };
}
test('single submission, polling delay, exact result and durable cache across restart', async t => {
  const f = fixture(t);
  const [a,b] = await Promise.all([f.relay.prove(input()), f.relay.prove(input())]);
  assert.deepEqual(a,b); assert.deepEqual(f.waits,[10_000]); assert.equal(f.calls.length,2);
  const first = f.calls[0];
  assert.equal(first.url, 'https://api.starkscan.co/v1/SN_MAIN/prove');
  assert.deepEqual(JSON.parse(first.init.body),input());
  assert.equal(first.init.redirect,'error'); assert.equal(first.init.headers['X-Starkscan-Api-Key'],'synthetic-test-key');
  assert.match(first.init.headers['Idempotency-Key'],/^[a-f0-9-]{36}$/);
  assert.deepEqual(await f.restart().prove(input()),a); assert.equal(f.calls.length,2);
  assert.equal(f.relay.status().local_attempts,1);
  const disk=readFileSync(join(f.directory,'relay.sqlite'));
  assert.ok(disk.includes(Buffer.from('synthetic-proof')));
  assert.ok(!disk.includes(Buffer.from('synthetic-test-key')));
  assert.ok(!disk.includes(Buffer.from('0xdeadbeef0123456789')));
  assert.equal(statSync(join(f.directory,'relay.sqlite')).mode & 0o777,0o600);
});
test('UTC daily budget covers clients and survives restart', async t => {
  const f=fixture(t,(_url,_init,now,calls)=>json({...queued,jobId:'prv_'+calls.length.toString(16).padStart(8,'0'),status:'succeeded',terminal:true,result:proof(now())},202));
  for(let i=0;i<10;i++)await f.relay.prove(input(100+i));
  assert.equal(f.relay.status().local_remaining,0);
  await assert.rejects(f.restart().prove(input(200)),{code:-32062}); assert.equal(f.calls.length,10);
  f.setNow(Date.UTC(2026,8,30)); await f.relay.prove(input(200)); assert.equal(f.calls.length,11);
  assert.equal(f.relay.status().local_attempts,1);
});
test('lost POST response reuses the persisted idempotency key, never starts another request', async t => {
  const f=fixture(t,(_url,init,now,calls)=>{if(calls.length===1)throw new Error('private upstream detail');return json({...queued,status:'succeeded',terminal:true,result:proof(now())},200);});
  await assert.rejects(f.relay.prove(input()),{code:-32065});
  await assert.rejects(f.relay.prove(input(101)),{code:-32063});
  await f.restart().prove(input()); assert.equal(f.calls.length,2);
  assert.equal(f.calls[0].init.headers['Idempotency-Key'],f.calls[1].init.headers['Idempotency-Key']);
  assert.equal(f.relay.status().local_attempts,1);
});
test('unknown delivery persists, blocks another logical job, and redacts private error data', async t => {
  const f=fixture(t,()=>json({...queued,status:'unknown_delivery',terminal:true,error:{data:'secret-calldata'}}));
  const rpc=await f.relay.rpc({jsonrpc:'2.0',id:1,method:'starknet_proveTransaction',params:input()});
  assert.equal(rpc.error.code,-32064);assert.ok(!JSON.stringify(rpc).includes('secret-calldata'));
  assert.equal(rpc.error.data.job_id,queued.jobId);
  await assert.rejects(f.restart().prove(input(101)),{code:-32063});assert.equal(f.calls.length,1);
});
test('429 respects Retry-After, does not spin, and does not claim server remaining allowance', async t => {
  const f=fixture(t,()=>json({code:'prover_daily_budget_exhausted',message:'private'},429,{'Retry-After':'43200'}));
  await assert.rejects(f.relay.prove(input()),{code:-32062});
  await assert.rejects(f.relay.prove(input(101)),{code:-32063}); assert.equal(f.calls.length,1);
  assert.equal(f.relay.status().retry_after_seconds,43200);
  assert.equal(f.relay.status().local_attempts,1);
  assert.ok(!('server_remaining' in f.relay.status()));
});
test('disabled, unauthorized and expired/missing result paths never imply successful proving', async t => {
  for (const [status,code] of [[404,-32060],[403,-32061],[401,-32061]]) {
    const f=fixture(t,()=>json({message:'never echo'},status));
    await assert.rejects(f.relay.prove(input()),{code});assert.equal(f.calls.length,1);
  }
  const lost=fixture(t,()=>json({...queued,status:'succeeded',terminal:true,resultUnavailableReason:'delivered_or_expired'}));
  await assert.rejects(lost.relay.prove(input()),{code:-32069});
  const stale=fixture(t,(_u,_i,now)=>json({...queued,status:'succeeded',terminal:true,result:proof(now()-241_000)}));
  await assert.rejects(stale.relay.prove(input()),{code:-32066});
  await assert.rejects(stale.restart().prove(input()),{code:-32066});assert.equal(stale.calls.length,1);
});
test('a interrupted poll resumes by GET after restart without another POST', async t => {
  const f=fixture(t,(_url,init,now,calls)=>{
    if(init.method==='POST')return json(queued,202);
    if(calls.length===2)throw new Error('interrupted');
    return json({...queued,status:'succeeded',terminal:true,result:proof(now())});
  });
  await assert.rejects(f.relay.prove(input()),{code:-32065});
  await f.restart().resume();
  const result=await f.relay.prove(input()); assert.equal(result.proof,'synthetic-proof');
  assert.equal(f.calls.filter(c=>c.init.method==='POST').length,1);
});
test('bad requests never touch the network or consume allowance', async t => {
  const f=fixture(t);
  for(const params of [{}, {...input(),block_id:'latest'}, {...input(),transaction:{...input().transaction,tip:'0x1'}}]) {
    await assert.rejects(f.relay.prove(params),{code:-32602});
  }
  assert.equal(f.calls.length,0);assert.equal(f.relay.status().local_attempts,0);
  assert.throws(()=>validateInput({...input(),transaction:{...input().transaction,version:'0x1'}}),{code:-32602});
  assert.throws(()=>freshScreening(proof(Date.now()+60_000)),{code:-32066});
  assert.throws(()=>freshScreening({}),{code:-32066});
});
test('browser origins, DNS rebinding and non-JSON requests cannot spend the allowance',()=>{
  const req={method:'POST',url:'/',headers:{host:'127.0.0.1:3001','content-type':'application/json'}};
  assert.ok(allowedRequest(req));
  for(const headers of [{origin:'https://example.com'},{host:'example.com:3001'},{'content-type':'text/plain'},{'sec-fetch-site':'cross-site'}]) {
    assert.ok(!allowedRequest({...req,headers:{...req.headers,...headers}}));
  }
  assert.ok(!allowedRequest({...req,method:'GET'}));assert.ok(!allowedRequest({...req,url:'/other'}));
});
