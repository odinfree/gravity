import { useEffect, useRef, useState } from "react";
import { api, type Account, type Status, type PrivacySettings, type PrivacyStatus, type PrivacyReview, type PrivacySubmission, type PrivacyReceipt } from "../api";
import { parseStrk, formatStrk } from "../privacy-amount";

type Operation = "register" | "deposit" | "transfer" | "withdraw";
const labels: Record<Operation,string> = {register:"Register",deposit:"Shield",transfer:"Private transfer",withdraw:"Unshield"};
export function Privacy({status}: {status: Status}) {
  const [accounts,setAccounts]=useState<Account[]>([]);
  const [selected,setSelected]=useState("");
  const [settings,setSettings]=useState<PrivacySettings|null>(null);
  const [runtime,setRuntime]=useState(false);
  const [editing,setEditing]=useState(false);
  const [data,setData]=useState<PrivacyStatus|null>(null);
  const [operation,setOperation]=useState<Operation>("deposit");
  const [quantity,setQuantity]=useState("10");
  const [recipient,setRecipient]=useState("");
  const [busy,setBusy]=useState("");
  const [error,setError]=useState("");
  const [balanceError,setBalanceError]=useState("");
  const [review,setReview]=useState<PrivacyReview|null>(null);
  const [submitted,setSubmitted]=useState<PrivacySubmission|null>(null);
  const [receipt,setReceipt]=useState<PrivacyReceipt|null>(null);
  const [history,setHistory]=useState<PrivacySubmission[]>([]);
  const generation=useRef(0);
  const network=status.network==="SN_MAIN"?"mainnet":"testnet";
  const chainId=network==="mainnet"?"0x534e5f4d41494e":"0x534e5f5345504f4c4941";
  const context={account:selected,chain_id:chainId};
  useEffect(()=>{
    let active=true;
    api.listAccounts().then(a=>{if(active){const users=a.filter(v=>v.domain==="user");setAccounts(users);setSelected(users[0]?.address??"");}}).catch(e=>{if(active)setError(String(e));});
    api.privacySettings().then(s=>{if(active){setSettings(s.settings);setRuntime(s.runtime_ready);}}).catch(e=>{if(active)setError(String(e));});
    return()=>{active=false;generation.current++;};
  },[]);
  useEffect(()=>{
    generation.current++;setData(null);setReview(null);setSubmitted(null);setReceipt(null);setError("");setBalanceError("");setBusy("");
    let active=true;setHistory([]);
    if(selected)api.privacyHistory(selected,chainId).then(h=>{if(active)setHistory(h);}).catch(e=>{if(active)setError(String(e));});
    return()=>{active=false;};
  },[selected,chainId]);

  async function task(label:string, action:(epoch:number)=>Promise<void>) {
    const epoch=generation.current;setError("");setBusy(label);
    try{await action(epoch);}catch(e){if(epoch===generation.current)setError(String(e));}
    finally{if(epoch===generation.current)setBusy("");}
  }
  function refresh(discover=false){return task(discover?"Discovering shielded notes…":"Checking account…",async epoch=>{
    try {
      const value=await api.privacyStatus({...context,mode:discover?"balances":"status"});
      if(epoch===generation.current){setData(value);if(discover)setBalanceError("");}
    } catch(e) {
      if(!discover)throw e;
      if(epoch===generation.current)setBalanceError("Shielded balance unavailable. Your last confirmed transaction is unchanged.");
    }
  });}
  function prepare(op:Operation){return task("Preparing your proof and fees…",async epoch=>{
    const amount=op==="register"?"0":parseStrk(quantity);
    const target=op==="withdraw"?(recipient.trim()||selected):recipient.trim();
    if(op==="transfer" && !/^0x[0-9a-fA-F]+$/.test(target))throw new Error("Enter the recipient’s Starknet address.");
    let value:PrivacyReview;
    try { value=await api.privacyPrepare({...context,mode:"prepare",operation:op,amount,recipient:target||selected}); }
    catch(e) {
      // Fetch the backend's observed screening state; never infer readiness from health.
      if(op==="deposit")try {
        const current=await api.privacyStatus({...context,mode:"status"});
        if(epoch===generation.current)setData(current);
      } catch { /* Keep the original actionable error. */ }
      throw e;
    }
    if(epoch!==generation.current)return;
    setReview(value);setSubmitted(null);setReceipt(null);setBusy("Approve the amount and fees in the wallet…");
    // One action starts the flow. Rust still requires the final human approval.
    try {
      const sent=await api.privacySubmit(value.review_id);
      if(epoch===generation.current){setSubmitted(sent);setHistory(h=>[sent,...h]);}
    } finally {if(epoch===generation.current)setReview(null);}
  });}
  function checkReceipt(){if(!submitted)return;return task("Checking the transaction receipt…",async epoch=>{
    const value=await api.privacyReceipt(submitted.transaction_hash,submitted.chain_id);
    if(epoch===generation.current)setReceipt(value);
  });}
  function save(){if(!settings)return;return task("Saving privacy services…",async epoch=>{
    await api.setPrivacySettings(settings);
    if(epoch===generation.current){setEditing(false);setData(null);setReview(null);}
  });}
  function cancelSettings(){return task("Restoring privacy services…",async epoch=>{
    const current=await api.privacySettings();
    if(epoch===generation.current){setSettings(current.settings);setEditing(false);}
  });}
  // Check registration automatically on account/network changes and after service edits.
  useEffect(()=>{
    if(!selected||!runtime||editing)return;
    const epoch=generation.current;let active=true;
    api.privacyStatus({...context,mode:"status"}).then(value=>{if(active&&epoch===generation.current)setData(value);}).catch(e=>{if(active&&epoch===generation.current)setError(String(e));});
    return()=>{active=false;};
  },[selected,chainId,runtime,editing]);
  // Accepted registration must age past the proof base before dependent actions.
  useEffect(()=>{
    if(!data?.registered||data.registration_mature||busy)return;
    const timer=setTimeout(()=>{void refresh();},5000);return()=>clearTimeout(timer);
  },[data,busy]);
  // Reconcile without another button click. Never re-submit on timeouts.
  useEffect(()=>{
    if(!submitted)return;
    let active=true;let timer:ReturnType<typeof setTimeout>|undefined;
    const epoch=generation.current;
    const poll=async()=>{
      try {
        const value=await api.privacyReceipt(submitted.transaction_hash,submitted.chain_id);
        if(!active||epoch!==generation.current)return;
        setReceipt(value);
        if(["ACCEPTED_ON_L2","ACCEPTED_ON_L1"].includes(value.finality_status??"")){
          setHistory(h=>h.map(v=>v.transaction_hash===submitted.transaction_hash?{...v,status:value.execution_status??v.status}:v));
          // Reconcile public state separately: unavailable discovery must not hide
          // registration or restart polling an already-final receipt.
          try {
            const current=await api.privacyStatus({...context,mode:"status"});
            if(active&&epoch===generation.current)setData(current);
          } catch { /* The receipt still establishes this transaction's outcome. */ }
          try {
            const balances=await api.privacyStatus({...context,mode:"balances"});
            if(active&&epoch===generation.current){setData(balances);setBalanceError("");}
          } catch {
            if(active&&epoch===generation.current)setBalanceError("Transaction confirmed. Shielded balance refresh is unavailable; try Refresh balance later.");
          }
          return;
        }
      } catch { /* A missing receipt is not a rejection; retry reads only. */ }
      if(active)timer=setTimeout(poll,5000);
    };
    void poll();return()=>{active=false;if(timer)clearTimeout(timer);};
  },[submitted?.transaction_hash]);
  const disabled=Boolean(busy)||!selected||!runtime;
  const accepted=receipt?.execution_status==="SUCCEEDED"&&["ACCEPTED_ON_L2","ACCEPTED_ON_L1"].includes(receipt.finality_status??"");
  const screeningMissing=data?.deposit_screening==="signature_missing";
  const screeningRequired=(settings?.[network].screening_policy??"required")==="required";
  const hosted=settings?.[network].deposit_prover==="starkscan";
  const relay=data?.hosted_prover;
  const hostedBlocked=hosted&&(!relay?.reachable||relay.local_remaining===0||Boolean(relay.pending?.length)||Boolean(relay.retry_after_seconds));
  return <section className="panel privacy-panel">
    <div className="privacy-title"><h2>Privacy</h2><span className="badge">{network==="mainnet"?"Mainnet":"Sepolia"}</span></div>
    <p className="muted small">Starknet privacy with your chosen services. The current adapter supports STRK20-compatible pools and STRK.</p>
    <label htmlFor="privacy-account">Account</label>
    <select id="privacy-account" className="input" value={selected} disabled={Boolean(busy)} onChange={e=>setSelected(e.target.value)}>
      {accounts.map(a=><option key={a.address} value={a.address}>{a.label||"Account"} · {a.address.slice(0,10)}…</option>)}
    </select>
    {!accounts.length&&<p className="muted">Create an account in Accounts to get started.</p>}
    {!runtime&&<p className="error">Privacy runtime unavailable. Install Node.js 24+ and build the bundled privacy worker.</p>}
    <div className="privacy-actions"><button className="ghost" disabled={disabled} onClick={()=>refresh(true)}>Refresh balance</button>
      <button className="ghost" disabled={Boolean(busy)} onClick={()=>{setReview(null);if(editing)void cancelSettings();else setEditing(true);}}>Privacy services</button></div>
    {editing&&settings&&<div className="privacy-card">
      <h3>Services for {network==="mainnet"?"Mainnet":"Sepolia"}</h3>
      <p className="muted small">Use a discovery service you trust: it receives viewing material. The prover and blockchain node use the endpoints in Settings.</p>
      <label htmlFor="privacy-pool">Pool address</label><input id="privacy-pool" className="input" value={settings[network].pool_address} onChange={e=>setSettings({...settings,[network]:{...settings[network],pool_address:e.target.value}})}/>
      <label htmlFor="privacy-policy">Deposit policy</label><select id="privacy-policy" className="input" value={settings[network].screening_policy} onChange={e=>setSettings({...settings,[network]:{...settings[network],screening_policy:e.target.value as "required"|"pool_enforced"}})}><option value="required">Screening attestation required</option><option value="pool_enforced">Custom pool — policy enforced by its contract</option></select><p className="muted small">Changing this setting cannot override a pool’s on-chain rules. Official STRK20 pools require screening.</p>
      <label htmlFor="privacy-discovery">Discovery endpoint</label><input id="privacy-discovery" className="input" placeholder="http://127.0.0.1:8080" value={settings[network].discovery_url} onChange={e=>setSettings({...settings,[network]:{...settings[network],discovery_url:e.target.value}})}/>
      {network==="mainnet"&&<><label htmlFor="privacy-deposit-prover">Shielding prover</label><select id="privacy-deposit-prover" className="input" value={settings[network].deposit_prover??"configured"} onChange={e=>setSettings({...settings,[network]:{...settings[network],deposit_prover:e.target.value as "configured"|"starkscan"}})}><option value="configured">Use the prover in Settings</option><option value="starkscan">Starkscan — screened mainnet deposits</option></select><p className="muted small">Starkscan receives the deposit’s private proving inputs. The shared local adapter keeps its key outside the wallet and limits hosted attempts to 10 per UTC day. Registration, transfers and withdrawals keep using your configured prover.</p></>}
      <div className="privacy-actions"><button className="ghost" disabled={Boolean(busy)} onClick={()=>cancelSettings()}>Back</button><button className="primary" disabled={Boolean(busy)} onClick={save}>Save privacy services</button></div>
    </div>}
    {data&&<div className="privacy-card">
      <dl className="privacy-balances"><dt>Public STRK</dt><dd title={data.public_balance}>{formatStrk(data.public_balance)}</dd>
        <dt>Shielded STRK</dt><dd>{data.shielded_balance===undefined?"Refresh to discover":formatStrk(data.shielded_balance)}</dd>
        <dt>Spendable STRK</dt><dd>{data.spendable_balance===undefined?"—":formatStrk(data.spendable_balance)}</dd>
        <dt>Pool fee per operation</dt><dd>{formatStrk(data.pool_fee)} STRK</dd>
        <dt>Registration</dt><dd>{data.registered?(data.registration_mature?"Registered":"Registered · settling…"):"Not registered with this pool"}</dd>
        <dt>Shielding</dt><dd>{screeningMissing?"Unavailable · screening missing":screeningRequired?"Screening not verified":"Pool policy applies"}</dd></dl>
      {!data.registered&&<><p className="muted small">Register once to use this pool. Click Register, approve the fees once, and gravity handles the rest.</p><button className="primary" disabled={disabled} onClick={()=>prepare("register")}>Register</button></>}
      {hosted&&<p className="muted small" role="status">{!relay?.reachable?"Starkscan adapter is not running.":`Hosted attempts today: ${relay.local_attempts} / ${relay.local_limit} across local clients. Resets at 00:00 UTC. This counter does not include requests sent outside the adapter.`}{relay?.pending?.length?" A hosted proof is unresolved; wait or check the shared relay status.":""}{relay?.retry_after_seconds?` Retry after ${relay.retry_after_seconds} seconds.`:""}</p>}
    </div>}
    {data?.registered&&!review&&(!submitted||accepted||receipt?.execution_status==="REVERTED")&&<div className="privacy-card">
      <label htmlFor="privacy-operation">Action</label>
      <select id="privacy-operation" className="input" value={operation} disabled={Boolean(busy)} onChange={e=>setOperation(e.target.value as Operation)}>
        <option value="deposit">Shield STRK</option><option value="transfer">Private transfer</option><option value="withdraw">Unshield STRK</option>
      </select>
      <label htmlFor="privacy-amount">Amount in STRK</label><input id="privacy-amount" className="input" inputMode="decimal" value={quantity} disabled={Boolean(busy)} onChange={e=>setQuantity(e.target.value)}/>
      {operation!=="deposit"&&<><label htmlFor="privacy-recipient">{operation==="transfer"?"Registered recipient":"Public recipient (blank = this account)"}</label><input id="privacy-recipient" className="input" placeholder="0x…" value={recipient} disabled={Boolean(busy)} onChange={e=>setRecipient(e.target.value)}/></>}
      {operation==="deposit"&&<p className="muted small">{screeningMissing?"Direct shielding is unavailable for this pool. The selected prover returned no authorized screening signature; no deposit was submitted.":hosted?"Shielding uses Starkscan’s hosted prover and screening service. Each new proof attempt uses the shared allowance, even if you later cancel. Approve promptly: screening expires within five minutes.":screeningRequired?"This pool requires an authorized screening signature for every deposit. Use an authorized prover or select Starkscan in Privacy services with operator-issued access.":"Shielding makes a public deposit. This pool enforces its configured deposit policy."}</p>}
      {operation==="transfer"&&<p className="muted small">The recipient must be registered. Pool notes stay private, but submitting from this account reveals who paid the network fee.</p>}
      {operation==="withdraw"&&<p className="muted small">Unshielding reveals the amount and recipient. New notes need time to mature before spending.</p>}
      <button className="primary" disabled={disabled||!data.registration_mature||(operation==="deposit"&&(screeningMissing||hostedBlocked))} onClick={()=>prepare(operation)}>{operation==="deposit"?(screeningMissing||hostedBlocked?"Shielding unavailable":"Shield STRK"):labels[operation]}</button>
      {!data.registration_mature&&<p className="muted small">Registration is accepted. Waiting for the pool state to settle before your next action…</p>}
    </div>}
    {review&&<div className="privacy-card" aria-label="Privacy transaction review">
      <h3>Review {labels[review.operation as Operation]?.toLowerCase()||review.operation}</h3><p className="muted small">Adapter: {review.adapter} · Deposit policy: {review.screening_policy}</p>
      <dl className="privacy-balances"><dt>Amount</dt><dd>{formatStrk(review.amount)} STRK</dd><dt>Pool fee</dt><dd>{formatStrk(review.pool_fee)} STRK</dd><dt>Maximum network fee</dt><dd>{formatStrk(review.max_network_fee)} STRK</dd></dl>
      <p className="small">Recipient</p><code className="addr">{review.recipient}</code>
      {review.operation==="deposit"&&<p>{review.screening_attached?"Screening signature received.":"Custom pool policy · no screening attestation attached."}</p>}
      {review.warnings.map((w,i)=><p className="muted small" key={i}>{w}</p>)}
      <p className="muted small">This review expires in five minutes. The wallet will ask for final approval before spending.</p>
      <p className="muted small">Approve or reject in the wallet confirmation dialog.</p>
    </div>}
    {submitted&&<div className="privacy-card" role="status">
      {accepted?<><p>✓ Transaction accepted. You can continue.</p><details><summary>View receipt</summary><code className="addr">{submitted.transaction_hash}</code><p className="muted small">{receipt?.execution_status} · {receipt?.finality_status} · block {receipt?.block_number}</p></details></>:<>
        <h3>{receipt?.execution_status==="REVERTED"?"Transaction reverted":"Waiting for confirmation…"}</h3>
        <code className="addr">{submitted.transaction_hash}</code>
        <p className="muted small">{submitted.status==="submission_unknown"?"The network response was uncertain. Checking the hash automatically; no payment is being retried.":"gravity is checking the receipt automatically."}</p>
        {receipt&&<p>{receipt.execution_status} · {receipt.finality_status}</p>}
        <button className="ghost" disabled={Boolean(busy)} onClick={checkReceipt}>Check now</button>
      </>}
    </div>}
    {history.length>0&&<details className="privacy-card"><summary>Recent privacy transactions</summary>{history.map(h=><div key={h.transaction_hash}><code className="addr">{h.transaction_hash}</code><button className="ghost" disabled={Boolean(busy)} onClick={()=>{setSubmitted(h);setReceipt(null);}}>Open receipt check</button></div>)}</details>}
    {busy&&<p className="privacy-progress" role="status">{busy}</p>}
    {balanceError&&<p role="status" className="muted small">{balanceError}</p>}
    {error&&<div className="privacy-error" role="alert"><p>{error}</p><button className="ghost" disabled={Boolean(busy)} onClick={()=>{setError("");setReview(null);}}>Dismiss</button></div>}
  </section>;
}
