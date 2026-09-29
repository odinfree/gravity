import { useEffect, useState } from "react";
import type { PrivacyProgressStage } from "../api";

export type ProgressState = {
  stage: PrivacyProgressStage | "confirming" | "succeeded" | "failed";
  label: string;
  startedAt: number;
  finishedAt?: number;
  error?: string;
};
const stages = {
  preparing: [0, "Preparing transaction", "Checking your account, balance and private notes."],
  proving: [1, "Generating proof", "This can take several minutes. Keep gravity open; you only need to click once."],
  checking_fees: [2, "Checking proof and fees", "The proof is ready. Checking your fee limits before submission."],
  submitting: [2, "Submitting transaction", "Sending the transaction to Starknet once."],
  confirming: [3, "Waiting for confirmation", "Checking the on-chain receipt automatically. No repeat submission."],
  succeeded: [4, "Transaction confirmed", "Accepted on Starknet. Updating your balances."],
  failed: [-1, "Action needs attention", "Check the result before trying again."],
} as const;

export function PrivacyProgress({progress,onDismiss}:{progress:ProgressState;onDismiss:()=>void}) {
  const [now,setNow]=useState(Date.now());
  const terminal=progress.stage==="succeeded"||progress.stage==="failed";
  useEffect(()=>{
    if(terminal)return;
    const timer=setInterval(()=>setNow(Date.now()),1000);
    return()=>clearInterval(timer);
  },[terminal,progress.startedAt]);
  const seconds=Math.max(0,Math.floor(((progress.finishedAt??now)-progress.startedAt)/1000));
  const [step,title,detail]=stages[progress.stage];
  return <aside className={`privacy-live-progress ${terminal?progress.stage:"running"}`} aria-label="Transaction progress">
    <div className="privacy-progress-heading">
      <div role="status" aria-live="polite"><strong>{title}</strong><span>{progress.label}</span></div>
      <span className="privacy-elapsed" aria-label="Elapsed time">{Math.floor(seconds/60)}:{String(seconds%60).padStart(2,"0")}</span>
    </div>
    {!terminal&&<div className="privacy-progress-track" role="progressbar" aria-label={title} aria-valuetext={title}><span/></div>}
    <ol className="privacy-progress-steps" aria-label="Transaction stages">
      {["Prepare","Prove","Submit","Confirm"].map((label,i)=><li key={label} className={i<step?"done":i===step?"active":""} aria-current={i===step?"step":undefined}>{i<step?"✓ ":""}{label}</li>)}
    </ol>
    <p>{progress.error||detail}</p>
    {terminal&&<button className="ghost" onClick={onDismiss}>Dismiss status</button>}
  </aside>;
}
