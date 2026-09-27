// First pool adapter. Other pool ABIs/proof formats must supply their own adapter.
import { ec, hash } from 'starknet';
import { createPrivateTransfers, MAX_VIEWING_KEY } from '@starkware-libs/starknet-privacy-sdk';
import { STRK, felt, amount, endpoint, screeningRequired, approvalCalls, fail } from '../validation.mjs';

export async function run(request, callback) {
  const { config, account, operation, mode } = request;
  const address = felt(account), pool = felt(config.pool_address), token = felt(request.token ?? STRK);
  const chain = config.chain_id;
  if (!['0x534e5f4d41494e','0x534e5f5345504f4c4941'].includes(chain)) fail('CONFIG', 'Unsupported network');
  const vk = BigInt(request.viewing_key);
  if (vk < 1n || vk > MAX_VIEWING_KEY) fail('KEY', 'Invalid viewing key');
  const vkPublic = BigInt(ec.starkCurve.getStarkKey('0x'+vk.toString(16)));
  const rpcURL = endpoint(config.rpc_url), discoveryURL = endpoint(config.discovery_url);
  // SDK requests may go only to the configured node/discovery service. Redirects
  // are rejected so viewing material cannot be forwarded to a different host.
  const fetchOriginal = globalThis.fetch;
  globalThis.fetch = (input, init = {}) => {
    const url = new URL(typeof input === 'string' ? input : input.url ?? input.toString());
    const allowed = url.href === rpcURL.href || (url.origin === discoveryURL.origin &&
      url.pathname.startsWith(discoveryURL.pathname.replace(/\/$/, '') + '/'));
    if (!allowed) fail('DESTINATION', 'Privacy worker refused an unexpected network destination');
    return fetchOriginal(input, { ...init, redirect:'error', signal:init.signal ?? AbortSignal.timeout(30_000) });
  };
  const rpc = async (method, params) => {
    if (!['starknet_call','starknet_chainId','starknet_getClassHashAt','starknet_getNonce',
      'starknet_blockNumber','starknet_estimateFee','starknet_getBlockWithTxHashes'].includes(method)) fail('METHOD','Read-only node method required');
    const response = await fetch(config.rpc_url, {method:'POST', headers:{'Content-Type':'application/json'},
      body:JSON.stringify({jsonrpc:'2.0',id:1,method,params})});
    if (!response.ok) fail('NODE', 'Blockchain node request failed');
    const body = await response.json();
    if (body.error) fail('NODE', 'Blockchain node rejected '+method+' (code '+Number(body.error.code)+')');
    if (body.id !== 1 || !('result' in body)) fail('NODE','Invalid blockchain response');
    return body.result;
  };
  const call = async (to, name, calldata, block = 'latest') => rpc('starknet_call', {
    block_id:block, request:{contract_address:to,entry_point_selector:hash.getSelectorFromName(name),calldata}});
  if (felt(await rpc('starknet_chainId',[])) !== chain) fail('CHAIN','RPC network does not match the privacy configuration');
  const head = await rpc('starknet_blockNumber',[]), base = head - 10;
  if (!Number.isSafeInteger(base) || base < 0) fail('STATE','Cannot select a mature proof base');
  const block = {block_number:base};
  const registeredKey = BigInt((await call(pool,'get_public_key',[address]))[0]);
  if (registeredKey !== 0n && registeredKey !== vkPublic) fail('KEY_MISMATCH','This pool is registered with a different viewing key. Use the original wallet; do not overwrite registration.');
  const feeRaw = await call(pool,'get_fee_amount',[]);
  const fee = BigInt(feeRaw[0]);
  const balanceRaw = await call(token,'balanceOf',[address]);
  const publicBalance = BigInt(balanceRaw[0]) + (BigInt(balanceRaw[1] ?? 0) << 128n);
  const matureKey = BigInt((await call(pool,'get_public_key',[address],block))[0]);
  const common = {account:address,token,pool_address:pool,registered:registeredKey !== 0n,registration_mature:matureKey === vkPublic,
    viewing_key_matches:registeredKey !== 0n, pool_fee:fee.toString(),public_balance:publicBalance.toString(),
    adapter:'strk20-v2',screening_policy:config.screening_policy ?? 'required',proof_base:base,chain_id:chain};
  if (mode === 'status') return common;
  await rpc('starknet_getClassHashAt',{block_id:block,contract_address:address});
  const signer = {
    signTransaction: async (calls, details) => callback('sign', {
      calls:calls.map(c=>({...c,contractAddress:felt(c.contractAddress),calldata:c.calldata.map(v=>felt(String(v)))})),details}),
  };
  const transfers = createPrivateTransfers({account:{address,signer},
    viewingKeyProvider:{getViewingKey:async()=>vk},poolContractAddress:pool,
    discoveryProvider:{url:config.discovery_url},
    provingProvider:{
      getDefaultDetails:async()=>({walletAddress:pool,cairoVersion:'1',version:'0x3',chainId:chain,
        nonce:BigInt(await rpc('starknet_getNonce',{block_id:block,contract_address:pool})),
        tip:0n,paymasterData:[],accountDeploymentData:[],nonceDataAvailabilityMode:'L1',feeDataAvailabilityMode:'L1',
        resourceBounds:{l1_gas:{max_amount:1n,max_price_per_unit:0n},l2_gas:{max_amount:100_000_000n,max_price_per_unit:0n},l1_data_gas:{max_amount:1n,max_price_per_unit:0n}}}),
      prove:async(invocation)=>{
        const result=await callback('prove',{transaction:invocation,block_number:base});
        const message=result.l2_to_l1_messages?.find(m=>BigInt(m.from_address)===BigInt(pool));
        if (!result.proof || !result.proof_facts?.length || !message?.payload?.length) fail('PROOF','Prover returned no usable pool proof');
        return {data:result.proof,proofFacts:result.proof_facts,output:message.payload,additionalData:result.additional_data};
      },
    }});
  if (mode === 'balances') {
    if (!common.registered) return {...common,shielded_balance:'0',spendable_balance:'0',notes:0};
    const {notes}=await transfers.discoverNotes({tokens:[BigInt(token)],blockIdentifier:'latest'});
    const all=notes.get(BigInt(token)) ?? [];
    const mature=all.filter(n=>Number(n.created ?? Infinity) <= base);
    return {...common,shielded_balance:all.reduce((s,n)=>s+n.amount,0n).toString(),
      spendable_balance:mature.reduce((s,n)=>s+n.amount,0n).toString(),notes:all.length};
  }
  if (mode !== 'prepare' || !['register','deposit','transfer','withdraw'].includes(operation)) fail('INPUT','Unsupported privacy operation');
  if (operation === 'register' && common.registered) fail('REGISTERED','This account is already registered');
  if (operation !== 'register' && (!common.registered || matureKey !== vkPublic)) fail('REGISTER','Register this account first and wait for its registration to mature');
  const quantity = operation === 'register' ? 0n : amount(request.amount);
  const recipient = felt(request.recipient ?? address);
  if (operation === 'transfer' && BigInt((await call(pool,'get_public_key',[recipient],block))[0]) === 0n) fail('RECIPIENT','Recipient must register with this pool first');
  if (operation === 'deposit') {
    const oldBalance = await call(token,'balanceOf',[address],block);
    if (BigInt(oldBalance[0])+(BigInt(oldBalance[1] ?? 0)<<128n) < quantity) fail('MATURITY','Funding is insufficient or not yet available at the proof base');
  }
  let builder = operation === 'register' ? transfers.build() :
    transfers.build({autoSetup:true,autoDiscover:{notes:'refresh',channels:'refresh'}}).surplusTo(address);
  if (operation === 'register') builder=builder.register();
  else if (operation === 'deposit') builder=builder.with(token,t=>t.deposit({amount:quantity}));
  else {
    // Discover against the proof base and filter out notes without a known creation
    // height. Do not let auto-selection choose fresh or speculative notes.
    const {notes}=await transfers.discoverNotes({tokens:[BigInt(token)],blockIdentifier:base});
    const mature=(notes.get(BigInt(token)) ?? []).filter(n=>Number(n.created ?? Infinity) <= base);
    if (mature.reduce((s,n)=>s+n.amount,0n) < quantity) fail('BALANCE','Insufficient mature shielded balance');
    builder=builder.with(token,t=>{
      for (const note of mature) t.inputs(note);
      return operation === 'transfer' ? t.transfer({amount:quantity,recipient}) : t.withdraw({amount:quantity,recipient});
    });
  }
  const result = await builder.execute({provingBlockId:base});
  const {call:apply,proof}=result.callAndProof;
  screeningRequired(operation,proof,config.screening_policy);
  if (felt(apply.contractAddress)!==pool || apply.entrypoint!=='apply_actions') fail('PROOF','Unexpected pool call');
  const calls=[...approvalCalls(pool,fee,operation,token,quantity),
    {contract_address:pool,entrypoint:'apply_actions',calldata:apply.calldata.map(v=>felt(String(v)))}];
  const nonce=await rpc('starknet_getNonce',{block_id:'latest',contract_address:address});
  const zero={max_amount:'0x0',max_price_per_unit:'0x0'};
  const transaction={type:'INVOKE',version:'0x3',sender_address:address,nonce,
    calldata:[felt(String(calls.length)),...calls.flatMap(c=>[c.contract_address,hash.getSelectorFromName(c.entrypoint),felt(String(c.calldata.length)),...c.calldata])],
    signature:[],account_deployment_data:[],resource_bounds:{l1_gas:zero,l2_gas:zero,l1_data_gas:zero},tip:'0x0',paymaster_data:[],
    nonce_data_availability_mode:'L1',fee_data_availability_mode:'L1',proof_facts:proof.proofFacts,proof:proof.data};
  // Only the final public pool call/proof is sent for estimation. Never the
  // compile_actions virtual calldata, which contains the viewing key.
  const [estimate]=await rpc('starknet_estimateFee',{request:[transaction],simulation_flags:['SKIP_VALIDATE'],block_id:'latest'});
  if (estimate.unit !== 'FRI') fail('FEE','Unexpected fee currency');
  const bounds=Object.fromEntries(['l1_gas','l2_gas','l1_data_gas'].map(r=>[r,{
    max_amount:felt(((BigInt(estimate[r+'_consumed'])*150n+99n)/100n).toString()),
    max_price_per_unit:felt(((BigInt(estimate[r+'_price'])*150n+99n)/100n).toString())}]));
  const maxFee=Object.values(bounds).reduce((n,v)=>n+BigInt(v.max_amount)*BigInt(v.max_price_per_unit),0n);
  if (maxFee <= 0n) fail('FEE','Node returned an invalid fee estimate');
  return {...common,operation,amount:quantity.toString(),recipient,calls,nonce,resource_bounds:bounds,
    max_network_fee:maxFee.toString(),proof:proof.data,proof_facts:proof.proofFacts,
    warnings:operation==='register'?[]:['The submitting account and network fees remain public. Deposits and withdrawals also reveal their amount.'],
    screening_attached:Boolean(proof.additionalData?.signature)};
}
