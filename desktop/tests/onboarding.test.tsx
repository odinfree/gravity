import "./setup-dom";
import { afterEach, beforeEach, test } from "node:test";
import assert from "node:assert/strict";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Onboarding } from "../src/components/Onboarding";
import { api, calls } from "./mock-api";

let root: Root;
let host: HTMLDivElement;
let done: number;
const original = { ...api };
beforeEach(async () => {
  Object.assign(api, original);
  calls.length = 0;
  done = 0;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => { root.render(<Onboarding onDone={() => { done++; }} />); });
});
afterEach(async () => {
  await act(async () => { root.unmount(); });
  host.remove();
});
function button(text: string): HTMLButtonElement {
  const el = [...host.querySelectorAll("button")].find((b) => b.textContent?.includes(text));
  assert.ok(el, `missing button: ${text}`);
  return el;
}
async function click(text: string) { await act(async () => { button(text).click(); }); }
async function type(id: string, value: string) {
  const el = host.querySelector(`#${id}`) as HTMLInputElement | HTMLTextAreaElement;
  assert.ok(el);
  const prototype = el.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  await act(async () => {
    Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function submit() {
  await act(async () => {
    host.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  });
}
async function reachPassword() {
  await click("Create a new wallet");
  assert.ok(button("Continue to password").disabled);
  assert.equal(host.querySelector(".recovery-words"), null);
  await click("Show recovery phrase");
  assert.equal(host.querySelectorAll(".recovery-words li").length, 12);
  await act(async () => { (host.querySelector('input[type="checkbox"]') as HTMLInputElement).click(); });
  await click("Continue to password");
  assert.equal(host.querySelector(".recovery-words"), null);
}

test("import has a working Back button and returns to fresh-wallet creation", async () => {
  await click("Import a recovery phrase");
  await type("recovery-phrase", "synthetic abandoned input");
  await click("Back");
  assert.equal(host.querySelector("textarea"), null);
  assert.match(host.querySelector(".setup-progress")!.textContent!, /Back up/);
  assert.equal(calls.filter((c) => c.method === "cancel").length, 1);
  await click("Create a new wallet");
  assert.match(host.textContent!, /Back up your wallet/);
  assert.ok(!host.textContent!.includes("fixtureword"));
  assert.deepEqual(calls.map((c) => c.method), ["cancel", "generate"]);
});

test("Back from password preserves the phrase, clears passwords, and Back again cancels", async () => {
  await reachPassword();
  await type("wallet-password", "synthetic-password");
  await click("Back");
  assert.equal(host.querySelector(".recovery-words"), null);
  assert.equal(calls.filter((c) => c.method === "cancel").length, 0);
  await click("Continue to password");
  assert.equal((host.querySelector("#wallet-password") as HTMLInputElement).value, "");
  await click("Back");
  await click("Back");
  assert.ok(button("Create a new wallet"));
  assert.equal(calls.filter((c) => c.method === "cancel").length, 1);
  assert.equal(calls.filter((c) => c.method === "generate").length, 1);
  assert.equal(done, 0);
});

test("password mismatch does not finalize, matching passwords complete once", async () => {
  await reachPassword();
  await type("wallet-password", "synthetic-password");
  await type("wallet-password-confirm", "different-password");
  await submit();
  assert.match(host.querySelector('[role="alert"]')!.textContent!, /don't match/);
  assert.equal(calls.filter((c) => c.method === "finalize").length, 0);
  await type("wallet-password-confirm", "synthetic-password");
  await submit();
  assert.equal(calls.filter((c) => c.method === "finalize").length, 1);
  assert.equal(done, 1);
  assert.equal((host.querySelector("#wallet-password") as HTMLInputElement).value, "");
});

test("generation in progress disables alternate routes and duplicate requests", async () => {
  let resolve!: (value: string) => void;
  let attempts = 0;
  api.generate = () => { attempts++; return new Promise((r) => { resolve = r; }); };
  await click("Create a new wallet");
  assert.ok(button("Creating recovery phrase").disabled);
  assert.ok(button("Import a recovery phrase").disabled);
  await click("Creating recovery phrase");
  assert.equal(attempts, 1);
  await act(async () => { resolve("fixtureword"); });
  assert.ok(!button("Back").disabled);
});

test("cancel failure retains the step and supports retry without losing the staged phrase", async () => {
  await click("Create a new wallet");
  api.cancelSetup = async () => { throw new Error("Synthetic cancel failure"); };
  await click("Back");
  assert.match(host.textContent!, /Back up your wallet/);
  assert.match(host.querySelector('[role="alert"]')!.textContent!, /cancel failure/);
  api.cancelSetup = original.cancelSetup;
  await click("Back");
  assert.ok(button("Create a new wallet"));
});

test("import errors allow correction, and Back from its password screen clears the import", async () => {
  await click("Import a recovery phrase");
  await type("recovery-phrase", "synthetic invalid input");
  api.import = async () => { throw new Error("invalid mnemonic"); };
  await submit();
  assert.match(host.querySelector('[role="alert"]')!.textContent!, /invalid mnemonic/);
  assert.ok(!button("Back").disabled);
  api.import = original.import;
  await submit();
  assert.ok(button("Import wallet"));
  assert.equal(host.querySelector("textarea"), null);
  await click("Back");
  assert.equal((host.querySelector("textarea") as HTMLTextAreaElement).value, "");
  assert.equal(calls.filter((c) => c.method === "cancel").length, 1);
  assert.equal(done, 0);
});
