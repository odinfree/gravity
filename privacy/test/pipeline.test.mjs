// Real bundled SDK + stdio protocol. Synthetic chain/proof; never mainnet.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {spawn} from 'node:child_process';
import {createInterface} from 'node:readline';
import {Signer,ec,hash} from 'starknet';
import {STRK} from '../worker.mjs';
const vk='42',pool='0x123',account='0x456',chain='0x534e5f4d41494e';
const publicKey=ec.starkCurve.getStarkKey('0x2a');
async function pipeline(operation,{screened=false,wrongChain=false,noteAt=900,policy='required',mode='prepare',discoveryFails=false}={}) {
 const methods=[],callbacks=[],requests=[];
 const server=createServer(async(req,res)=>{
  const chunks=[];for await(const c of req)chunks.push(c);
  const input=JSON.parse(Buffer.concat(chunks));requests.push({url:req.url,input});let value;
  if(req.url==='/rpc') {
   const {method,params}=input;methods.push(method);
   if(method==='starknet_chainId')value=wrongChain?'0x534e5f5345504f4c4941':chain;
   else if(method==='starknet_blockNumber')value=1000;
   else if(method==='starknet_getClassHashAt')value='0x99';
   else if(method==='starknet_getNonce')value='0xa';
   else if(method==='starknet_call'){
    const selector=params.request.entry_point_selector;
    if(selector===hash.getSelectorFromName('get_public_key'))value=[operation==='register'?'0x0':publicKey];
    else if(selector===hash.getSelectorFromName('get_fee_amount'))value=['0x53444835ec580000'];
    else if(selector===hash.getSelectorFromName('balanceOf'))value=['0x56bc75e2d63100000','0x0'];
    else throw new Error('Unexpected call');
   } else if(method==='starknet_estimateFee') {
    assert.ok(!JSON.stringify(params).includes(hash.getSelectorFromName('compile_actions')));
    assert.deepEqual(params.request[0].account_deployment_data,[]);assert.equal(params.request[0].proof,'synthetic-proof');assert.deepEqual(params.request[0].proof_facts,['0x1']);
    value=[{unit:'FRI',l1_gas_consumed:'0x1',l1_gas_price:'0x2',l2_gas_consumed:'0xa',l2_gas_price:'0x3',l1_data_gas_consumed:'0x1',l1_data_gas_price:'0x4'}];
   } else throw new Error('Unexpected node method '+method);
   value={jsonrpc:'2.0',id:input.id,result:value};
  } else if(req.url==='/v1/sync/outgoing_state') {
   assert.equal(input.block_ref,990);
   value={block_ref:990,channels:[{recipient_addr:account,recipient_public_key:publicKey,channel_key:'0x0',precomputed:true}],subchannels:[],cursor:{channel_discovery_complete:true,total_n_channels:0,channels:{}}};
  } else if(req.url==='/v1/sync/incoming_state') {
   assert.equal(input.block_ref,mode==='balances'?1000:990);
   if(discoveryFails){res.writeHead(503,{'Content-Type':'application/json'});res.end(JSON.stringify({error:'synthetic discovery failure'}));return;}
   value={block_ref:990,channels:[{sender_addr:'0x789',channel_key:'0x7b'}],
    notes:[{sender_addr:'0x789',token:STRK,note_id:'0xabc',amount:'12000000000000000000',block_number:noteAt,index:0,salt:'2'}],
    cursor:{channel_discovery_complete:true,total_n_channels:1,channels:{}}};
  } else throw new Error('Unexpected discovery route');
  res.writeHead(200,{'Content-Type':'application/json'});res.end(JSON.stringify(value));
 });
 await new Promise(r=>server.listen(0,'127.0.0.1',r));const base=`http://127.0.0.1:${server.address().port}`;
 const child=spawn(process.execPath,['../desktop/src-tauri/resources/privacy/worker.cjs'],{stdio:['pipe','pipe','pipe']});
 let stderr='';child.stderr.on('data',v=>stderr+=v);const exit=new Promise(r=>child.on('exit',r));
 const timer=setTimeout(()=>child.kill(),15_000);let result;
 try {
  child.stdin.write(JSON.stringify({config:{screening_policy:policy,pool_address:pool,chain_id:chain,rpc_url:base+'/rpc',discovery_url:base},account,viewing_key:vk,mode,operation,amount:'10000000000000000000',recipient:account})+'\n');
  const signer=new Signer('0x1'); // public throwaway test scalar
  for await(const line of createInterface({input:child.stdout})){
   const message=JSON.parse(line);callbacks.push(message.kind);
   if(message.kind==='sign') {
    assert.equal(message.payload.details.nonce,'10');
    const details=message.payload.details;
    details.resourceBounds=Object.fromEntries(Object.entries(details.resourceBounds).map(([k,v])=>[k,{max_amount:BigInt(v.max_amount),max_price_per_unit:BigInt(v.max_price_per_unit)}]));
    const signature=await signer.signTransaction(message.payload.calls,details);
    child.stdin.write(JSON.stringify({id:message.id,result:signature},(_,v)=>typeof v==='bigint'?'0x'+v.toString(16):v)+'\n');
   } else if(message.kind==='prove'){
    assert.equal(message.payload.block_number,990);assert.equal(message.payload.transaction.sender_address,pool);assert.equal(message.payload.transaction.nonce,'0xa');
    const proof={proof:'synthetic-proof',proof_facts:['0x1'],l2_to_l1_messages:[{from_address:pool,payload:['0x9','0x0']}]};
    if(screened)proof.additional_data={signature:{sig_r:'0x1',sig_s:'0x2',issued_at:12345}};
    child.stdin.write(JSON.stringify({id:message.id,result:proof})+'\n');
   } else result=message;
  }
  await exit;assert.equal(stderr,'');assert.ok(methods.every(m=>!m.startsWith('starknet_add')));
  return {result,callbacks,methods,requests};
 } finally {clearTimeout(timer);child.kill();server.closeAllConnections();await new Promise(r=>server.close(r));}
}
test('real SDK registration uses mature block, nonce 10 and public proof estimation',async()=>{
 const {result,callbacks,requests}=await pipeline('register');assert.deepEqual(callbacks,['sign','prove','result']);assert.equal(result.kind,'result');
 assert.equal(result.result.calls[0].calldata[1],'0x53444835ec580000');assert.equal(result.result.max_network_fee,'93');
 assert.ok(requests.every(r=>r.url==='/rpc'),'registration must not use discovery');
});
test('real SDK deposit stops before fee estimation without screening',async()=>{
 const {result,methods}=await pipeline('deposit');assert.equal(result.code,'SCREENING_REQUIRED');assert.ok(!methods.includes('starknet_estimateFee'));
});
test('screened deposit produces exact allowance and final attestation',async()=>{
 const {result}=await pipeline('deposit',{screened:true});assert.equal(result.kind,'result');assert.equal(result.result.screening_attached,true);
 assert.equal(BigInt(result.result.calls[0].calldata[1]),16n*10n**18n);assert.deepEqual(result.result.calls[1].calldata.slice(-4),['0x0','0x3039','0x1','0x2']);
});
test('wrong-chain node stops before signing',async()=>{
 const {result,callbacks}=await pipeline('register',{wrongChain:true});assert.equal(result.code,'CHAIN');assert.deepEqual(callbacks,['error']);
});

test('custom compatible pool policy can prepare without a screening attestation',async()=>{
 const {result}=await pipeline('deposit',{policy:'pool_enforced'});assert.equal(result.kind,'result');assert.equal(result.result.screening_attached,false);
});
for(const operation of ['transfer','withdraw'])test(`real SDK ${operation} spends mature discovered notes and approves only the pool fee`,async()=>{
 const {result,callbacks}=await pipeline(operation);assert.equal(result.kind,'result');assert.deepEqual(callbacks,['sign','prove','result']);
 assert.equal(BigInt(result.result.calls[0].calldata[1]),6n*10n**18n);
});
test('fresh discovered notes cannot be selected for a private transfer',async()=>{
 const {result,callbacks}=await pipeline('transfer',{noteAt:995});assert.equal(result.code,'BALANCE');assert.deepEqual(callbacks,['error']);
});
test('balance discovery is pinned to head and separates fresh from spendable notes',async()=>{
 const {result,callbacks}=await pipeline('deposit',{mode:'balances',noteAt:995});
 assert.deepEqual(callbacks,['result']);assert.equal(result.result.shielded_balance,'12000000000000000000');assert.equal(result.result.spendable_balance,'0');
});
test('balance discovery errors have a safe distinct code and never request a signature',async()=>{
 const {result,callbacks}=await pipeline('deposit',{mode:'balances',discoveryFails:true});
 assert.deepEqual(callbacks,['error']);assert.equal(result.code,'DISCOVERY');
});
