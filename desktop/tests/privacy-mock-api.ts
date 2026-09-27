export const calls: {method:string;value?:any}[]=[];
export const api={
  listAccounts:async()=>[{address:'0x123',label:'First',domain:'user'},{address:'0x456',label:'Second',domain:'user'}],
  privacyHistory:async()=>[],
  privacySettings:async()=>({runtime_ready:true,settings:{mainnet:{screening_policy:'required',pool_address:'0x789',discovery_url:'http://127.0.0.1:8080'},testnet:{screening_policy:'required',pool_address:'0x999',discovery_url:''}}}),
  setPrivacySettings:async(value:any)=>{calls.push({method:'settings',value});},
  privacyStatus:async(value:any)=>{calls.push({method:'status',value});return{registered:true,registration_mature:true,public_balance:'100000000000000000000',pool_fee:'6000000000000000000',proof_base:100};},
  privacyPrepare:async(value:any)=>{calls.push({method:'prepare',value});return{review_id:'synthetic-review',operation:value.operation,amount:value.amount,recipient:value.recipient,account:value.account,chain_id:value.chain_id,pool_fee:'6000000000000000000',max_network_fee:'100000000000000000',proof_base:100,screening_attached:true,warnings:[]};},
  privacySubmit:async(value:any)=>{calls.push({method:'submit',value});return{transaction_hash:'0xabc',status:'submitted',chain_id:'0x534e5f4d41494e'};},
  privacyReceipt:async()=>({transaction_hash:'0xabc',execution_status:'SUCCEEDED',finality_status:'ACCEPTED_ON_L2',block_number:110,actual_fee:{amount:'0x1',unit:'FRI'}}),
};
