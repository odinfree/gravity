// Wallet-owned stdio runtime. No HTTP listener, bearer token or signing key.
import { createInterface } from 'node:readline';
import { PrivacyError, fail } from './validation.mjs';
import { run as strk20 } from './adapters/strk20-v2.mjs';
export { amount, felt, endpoint, screeningRequired, approvalCalls, STRK } from './validation.mjs';

export async function run(request, callback) {
  const adapter=request.config?.adapter ?? 'strk20-v2';
  if(adapter !== 'strk20-v2') fail('ADAPTER','This pool adapter is not installed');
  return strk20(request,callback);
}

export async function main() {
  console.log=console.debug=console.info=console.warn=console.error=()=>{};
  const lines=createInterface({input:process.stdin,crlfDelay:Infinity})[Symbol.asyncIterator]();
  const send=value=>process.stdout.write(JSON.stringify(value,(_,v)=>typeof v==='bigint'?v.toString():v)+'\n');
  let sequence=0;
  const callback=async(kind,payload)=>{
    const id=++sequence; send({kind,id,payload});
    const next=await lines.next(); if(next.done) fail('PARENT','Wallet closed the request');
    const response=JSON.parse(next.value);
    if(response.id!==id || response.error) fail('PARENT','Wallet rejected the privacy operation');
    return response.result;
  };
  try {
    if(Number(process.versions.node.split('.')[0])<24) fail('RUNTIME','Node.js 24 or newer is required');
    const first=await lines.next();
    if(first.done || first.value.length>128*1024) fail('INPUT','Invalid privacy request');
    const result=await run(JSON.parse(first.value),callback);
    send({kind:'result',result});
  } catch(error) {
    send({kind:'error',code:error instanceof PrivacyError?error.code:'SERVICE',
      message:error instanceof PrivacyError?error.message:'Privacy operation failed. Check the configured node, discovery and prover services; details withheld to protect private inputs.'});
    process.exitCode=1;
  } finally { process.stdin.destroy(); }
}
