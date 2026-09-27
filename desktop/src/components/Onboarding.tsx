import { useRef, useState } from "react";
import { api } from "../api";

type Step = "welcome" | "backup" | "import" | "password";

export function Onboarding({ onDone }: { onDone: () => void }) {
  const [step, setStep] = useState<Step>("welcome");
  const [source, setSource] = useState<"new" | "import">("new");
  const [mnemonic, setMnemonic] = useState("");
  const [phrase, setPhrase] = useState("");
  const [revealed, setRevealed] = useState(false);
  const [hasRevealed, setHasRevealed] = useState(false);
  const [backedUp, setBackedUp] = useState(false);
  const [pass, setPass] = useState("");
  const [pass2, setPass2] = useState("");
  const [busy, setBusy] = useState(false);
  const working = useRef(false);
  const [err, setErr] = useState("");

  async function run(action: () => Promise<void>) {
    if (working.current) return;
    working.current = true;
    setBusy(true);
    setErr("");
    try { await action(); } catch (e) { setErr(String(e)); }
    finally { working.current = false; setBusy(false); }
  }

  function clearPassword() { setPass(""); setPass2(""); }

  function create() {
    void run(async () => {
      const words = await api.generate(12);
      setMnemonic(words);
      setPhrase("");
      setSource("new");
      setRevealed(false);
      setHasRevealed(false);
      setBackedUp(false);
      clearPassword();
      setStep("backup");
    });
  }

  function back() {
    if (step === "password" && source === "new") {
      clearPassword();
      setErr("");
      setRevealed(false);
      setStep("backup");
      return;
    }
    void run(async () => {
      // Discard the uncommitted Rust-side mnemonic as well as UI fields.
      await api.cancelSetup();
      setMnemonic("");
      setPhrase("");
      setRevealed(false);
      setHasRevealed(false);
      setBackedUp(false);
      clearPassword();
      setStep(step === "password" ? "import" : "welcome");
      if (step !== "password") setSource("new");
    });
  }

  function importPhrase() {
    void run(async () => {
      await api.import(phrase.trim());
      setPhrase("");
      setSource("import");
      setStep("password");
    });
  }

  function finalize() {
    setErr("");
    if (pass.length < 8) return setErr("Use at least 8 characters for your password.");
    if (pass !== pass2) return setErr("The passwords don't match. Please try again.");
    void run(async () => {
      await api.finalizeSetup(pass);
      setMnemonic("");
      setPhrase("");
      clearPassword();
      onDone();
    });
  }

  const stepNumber = step === "welcome" ? 1 : step === "password" ? 3 : 2;

  return (
    <main className="panel onboarding" aria-busy={busy}>
      <div className="setup-nav">
        {step !== "welcome" ? (
          <button className="back-button" onClick={back} disabled={busy}>← Back</button>
        ) : <span className="setup-eyebrow">YOUR LOCAL WALLET</span>}
        <span className="muted small">Step {stepNumber} of 3</span>
      </div>
      <ol className="setup-progress" aria-label="Wallet setup progress">
        {["Choose", source === "import" ? "Recovery phrase" : "Back up", "Password"].map((label, i) => (
          <li key={i} className={i + 1 <= stepNumber ? "active" : ""}
            aria-current={i + 1 === stepNumber ? "step" : undefined}>{label}</li>
        ))}
      </ol>

      {step === "welcome" && (
        <>
          <div className="setup-intro">
            <div className="wallet-mark" aria-hidden="true">＋</div>
            <h1>Create your wallet</h1>
            <p className="muted">Start fresh with a new recovery phrase. Your wallet is encrypted on this Mac.</p>
          </div>
          <button className="primary setup-create" onClick={create} disabled={busy}>
            {busy ? "Creating recovery phrase…" : "Create a new wallet"}
            <span aria-hidden="true">→</span>
          </button>
          <p className="setup-caption muted">Back up 12 words, then choose your password.</p>
          <div className="setup-divider"><span>Already have a wallet?</span></div>
          <button className="ghost" disabled={busy} onClick={() => {
            setSource("import"); setErr(""); setStep("import");
          }}>Import a recovery phrase</button>
          <div className="setup-note">
            <strong>Choose your network after setup</strong>
            <p>Switch between Mainnet and Sepolia in Accounts. Connect your own prover in Settings.</p>
          </div>
          <p className="setup-caption muted">Experimental software. Use a fresh test wallet.</p>
        </>
      )}

      {step === "backup" && (
        <>
          <h1>Back up your wallet</h1>
          <p className="muted">These 12 words recover your wallet. Write them down in order and keep them private.</p>
          <div className="recovery-card">
            {revealed ? (
              <ol className="recovery-words" aria-label="Recovery phrase">
                {mnemonic.split(" ").map((word, i) => <li key={i}><span>{i + 1}</span>{word}</li>)}
              </ol>
            ) : (
              <div className="recovery-hidden">
                <strong>Your recovery phrase is hidden</strong>
                <span className="muted">Reveal it when you are ready to write it down.</span>
              </div>
            )}
            <button className="ghost" disabled={busy} onClick={() => {
              setRevealed(!revealed); setHasRevealed(true);
            }}>{revealed ? "Hide recovery phrase" : "Show recovery phrase"}</button>
          </div>
          <label className="setup-check">
            <input type="checkbox" checked={backedUp} disabled={busy || !hasRevealed}
              onChange={(e) => setBackedUp(e.target.checked)} />
            <span>I saved my recovery phrase somewhere safe.</span>
          </label>
          <button className="primary" disabled={busy || !backedUp} onClick={() => {
            setRevealed(false); setErr(""); setStep("password");
          }}>Continue to password</button>
          <p className="setup-caption muted">Going back to the welcome screen discards this unfinished wallet.</p>
        </>
      )}

      {step === "import" && (
        <form className="setup-form" onSubmit={(e) => { e.preventDefault(); importPhrase(); }}>
          <h1>Import a wallet</h1>
          <p className="muted">Enter an existing recovery phrase. To create a fresh wallet, use Back.</p>
          <label className="field" htmlFor="recovery-phrase">Recovery phrase</label>
          <textarea id="recovery-phrase" className="input" rows={4}
            placeholder="12 or 24 words, separated by spaces" value={phrase}
            autoComplete="off" autoCapitalize="none" spellCheck={false} disabled={busy}
            onChange={(e) => setPhrase(e.target.value)} />
          <button className="primary" disabled={busy || !phrase.trim()}>
            {busy ? "Checking phrase…" : "Continue to password"}
          </button>
        </form>
      )}

      {step === "password" && (
        <form className="setup-form" onSubmit={(e) => { e.preventDefault(); finalize(); }}>
          <h1>Protect your wallet</h1>
          <p className="muted">Choose a password to encrypt and unlock your wallet on this Mac.</p>
          <label className="field" htmlFor="wallet-password">Password</label>
          <input id="wallet-password" className="input" type="password" autoComplete="new-password"
            placeholder="At least 8 characters" value={pass} minLength={8} required disabled={busy}
            onChange={(e) => setPass(e.target.value)} />
          <label className="field" htmlFor="wallet-password-confirm">Confirm password</label>
          <input id="wallet-password-confirm" className="input" type="password" autoComplete="new-password"
            placeholder="Enter your password again" value={pass2} required disabled={busy}
            onChange={(e) => setPass2(e.target.value)} />
          <button className="primary" disabled={busy || !pass || !pass2}>
            {busy ? "Encrypting wallet…" : source === "new" ? "Create wallet" : "Import wallet"}
          </button>
          <p className="setup-caption muted">Your password unlocks this app. Your recovery phrase restores the wallet.</p>
        </form>
      )}
      {err && <p className="error" role="alert">{err}</p>}
    </main>
  );
}
