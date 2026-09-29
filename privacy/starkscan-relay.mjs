/** Shared, loopback-only adapter for operator-issued Starkscan prove access.
 * Contract: https://starkscan.co/docs/api/strk20-prover
 * This adapter neither signs nor broadcasts. Private input is forwarded only to
 * the fixed official HTTPS host. One process serves every local wallet/client.
 */
import { createHash, randomUUID } from 'node:crypto';
import { mkdirSync, chmodSync, readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { setTimeout as sleep } from 'node:timers/promises';
import { pathToFileURL } from 'node:url';

const HOST = 'https://api.starkscan.co';
const ROUTE = '/v1/SN_MAIN/prove';
const PORT = 3001;
const MAX_INPUT = 1024 * 1024;
const MAX_OUTPUT = 32 * 1024 * 1024;
const MAINNET = '0x534e5f4d41494e';
const sha = value => createHash('sha256').update(value).digest('hex');
const canonical = value => Array.isArray(value) ? value.map(canonical) :
  value && typeof value === 'object' ? Object.fromEntries(Object.keys(value).sort().map(k => [k, canonical(value[k])])) : value;
const safeJob = value => typeof value === 'string' && /^prv_[a-fA-F0-9]{8,128}$/.test(value);
const messages = {
  [-32602]: 'Expected a signed, zero-fee Invoke v3 and an explicit block number.',
  [-32060]: 'Starkscan proving is not enabled at this endpoint.',
  [-32061]: 'Starkscan rejected access. Check the private key and operator-issued prove scope.',
  [-32062]: 'Hosted proving allowance exhausted. Wait until the UTC reset.',
  [-32063]: 'Another hosted proof is unresolved or the service is backing off. Check relay status.',
  [-32064]: 'Proof delivery is uncertain. Contact Starkscan with the job ID before starting another proof.',
  [-32065]: 'Hosted proof is pending or its response was interrupted. Retry the identical request to recover it.',
  [-32066]: 'Screening is missing, invalid, or too close to expiry. Nothing was broadcast.',
  [-32067]: 'The hosted prover rejected this request. Private diagnostics are saved locally.',
  [-32068]: 'Hosted proving is unavailable. Check relay status before trying again.',
  [-32069]: 'Invalid hosted response or private storage failure. Nothing was broadcast.',
};
export class RelayError extends Error {
  constructor(code, jobId) { super(messages[code] ?? messages[-32069]); this.code = code; if (safeJob(jobId)) this.jobId = jobId; }
}
const fail = (code, job) => { throw new RelayError(code, job); };

export function validateInput(params) {
  if (!params || Object.keys(params).sort().join(',') !== 'block_id,transaction' ||
      !Number.isSafeInteger(params.block_id?.block_number) || params.block_id.block_number < 0 ||
      Object.keys(params.block_id).join(',') !== 'block_number') fail(-32602);
  const tx = params.transaction;
  const field = v => typeof v === 'string' && /^(0x[0-9a-fA-F]+|[0-9]+)$/.test(v) && v.length < 80;
  if (!tx || tx.type !== 'INVOKE' || tx.version !== '0x3' || !field(tx.sender_address) || !field(tx.nonce) ||
      !Array.isArray(tx.calldata) || !tx.calldata.length || !tx.calldata.every(field) ||
      !Array.isArray(tx.signature) || tx.signature.length !== 2 || !tx.signature.every(field) ||
      !field(tx.tip) || BigInt(tx.tip) !== 0n ||
      !Array.isArray(tx.paymaster_data) || tx.paymaster_data.length ||
      (tx.account_deployment_data !== undefined && (!Array.isArray(tx.account_deployment_data) || tx.account_deployment_data.length))) fail(-32602);
  for (const name of ['l1_gas', 'l2_gas', 'l1_data_gas']) {
    const b = tx.resource_bounds?.[name];
    if (!b || !field(b.max_amount) || !field(b.max_price_per_unit) || BigInt(b.max_amount) * BigInt(b.max_price_per_unit) !== 0n) fail(-32602);
  }
  if (Buffer.byteLength(JSON.stringify(params)) > MAX_INPUT) fail(-32602);
}

export function freshScreening(result, now = Date.now()) {
  const sig = result?.additional_data?.signature;
  let issued;
  try { issued = Number(BigInt(sig?.issued_at)); } catch { fail(-32066); }
  if (!Number.isSafeInteger(issued) || issued <= 0 || issued > Math.floor(now / 1000) + 30 ||
      issued + 240 <= Math.floor(now / 1000) ||
      !/^0x[0-9a-fA-F]+$/.test(sig?.sig_r ?? '') || !/^0x[0-9a-fA-F]+$/.test(sig?.sig_s ?? '')) fail(-32066);
}

// All response/error bodies stay private. No arbitrary upstream text enters logs.
async function jsonBounded(response) {
  const parts = []; let size = 0;
  for await (const part of response.body ?? []) {
    size += part.length;
    if (size > MAX_OUTPUT) fail(-32069);
    parts.push(part);
  }
  try { return JSON.parse(Buffer.concat(parts).toString('utf8')); } catch { fail(-32069); }
}

export class StarkscanRelay {
  constructor({ apiKey, directory, dailyLimit = 10, fetchImpl = fetch, now = Date.now, wait = sleep }) {
    if (!apiKey || /\s/.test(apiKey) || !Number.isInteger(dailyLimit) || dailyLimit < 1 || dailyLimit > 10) fail(-32061);
    mkdirSync(directory, { recursive: true, mode: 0o700 }); chmodSync(directory, 0o700);
    const path = join(directory, 'relay.sqlite');
    this.db = new DatabaseSync(path); chmodSync(path, 0o600);
    this.db.exec(`PRAGMA busy_timeout=5000; PRAGMA synchronous=FULL;
      CREATE TABLE IF NOT EXISTS jobs (
        key_hash TEXT NOT NULL, digest TEXT NOT NULL, idem TEXT NOT NULL,
        day INTEGER NOT NULL, job TEXT, state TEXT NOT NULL, reply TEXT,
        PRIMARY KEY(key_hash, digest));
      CREATE TABLE IF NOT EXISTS backoff (key_hash TEXT PRIMARY KEY, until_ms INTEGER NOT NULL);`);
    this.key = apiKey; this.keyHash = sha(apiKey); this.limit = dailyLimit;
    this.fetch = fetchImpl; this.now = now; this.wait = wait; this.active = new Map();
  }
  close() { this.db.close(); }
  row(digest) { return this.db.prepare('SELECT * FROM jobs WHERE key_hash=? AND digest=?').get(this.keyHash, digest); }
  status() {
    const day = Math.floor(this.now() / 86_400_000);
    const used = this.db.prepare('SELECT count(*) AS n FROM jobs WHERE key_hash=? AND day=?').get(this.keyHash, day).n;
    const pending = this.db.prepare("SELECT job,state FROM jobs WHERE key_hash=? AND state IN ('reserved','queued','dispatched','unknown_delivery','unavailable')").all(this.keyHash);
    const until = this.db.prepare('SELECT until_ms FROM backoff WHERE key_hash=?').get(this.keyHash)?.until_ms ?? 0;
    return { service: 'gravity-starkscan-relay', chain_id: MAINNET,
      local_attempts: used, local_limit: this.limit, local_remaining: Math.max(0, this.limit - used),
      resets_at: new Date((day + 1) * 86_400_000).toISOString(),
      retry_after_seconds: Math.max(0, Math.ceil((until - this.now()) / 1000)),
      pending: pending.map(v => ({ job_id: v.job, status: v.state })) };
  }
  reserve(digest) {
    this.db.exec('BEGIN IMMEDIATE');
    try {
      const existing = this.row(digest);
      if (existing) { this.db.exec('COMMIT'); return existing; }
      const s = this.status();
      if (s.pending.length || s.retry_after_seconds) fail(-32063);
      if (!s.local_remaining) fail(-32062);
      this.db.prepare('INSERT INTO jobs(key_hash,digest,idem,day,state) VALUES(?,?,?,?,?)')
        .run(this.keyHash, digest, randomUUID(), Math.floor(this.now() / 86_400_000), 'reserved');
      this.db.exec('COMMIT'); return this.row(digest);
    } catch (e) { this.db.exec('ROLLBACK'); throw e; }
  }
  save(digest, reply) {
    if (!safeJob(reply.jobId) || !['queued','dispatched','succeeded','failed','unavailable','unknown_delivery'].includes(reply.status)) fail(-32069);
    if (reply.terminal !== !['queued','dispatched'].includes(reply.status)) fail(-32069);
    const previous = this.row(digest);
    if (previous.job && previous.job !== reply.jobId) fail(-32069);
    if (previous.reply) {
      const saved = JSON.parse(previous.reply);
      if (saved.result || saved.terminal) return saved;
    }
    // Synchronous FULL commit BEFORE polling again or returning the one-shot body.
    this.db.prepare('UPDATE jobs SET job=?,state=?,reply=? WHERE key_hash=? AND digest=?')
      .run(reply.jobId, reply.status, JSON.stringify(reply), this.keyHash, digest);
    return reply;
  }
  async http(path, init) {
    let response;
    try {
      response = await this.fetch(HOST + path, { ...init, redirect: 'error', signal: AbortSignal.timeout(60_000),
        headers: { 'X-Starkscan-Api-Key': this.key, 'Content-Type': 'application/json', ...init.headers } });
    } catch { fail(-32065); }
    if (!response.ok) {
      const after = response.headers.get('retry-after');
      let until = /^\d+$/.test(after ?? '') ? this.now() + Number(after) * 1000 : Date.parse(after ?? '');
      if (!Number.isFinite(until)) until = this.now() + (response.status === 429 ? 60_000 : 10_000);
      this.db.prepare('INSERT INTO backoff VALUES(?,?) ON CONFLICT(key_hash) DO UPDATE SET until_ms=max(until_ms,excluded.until_ms)')
        .run(this.keyHash, until);
      let body; try { body = await jsonBounded(response); } catch { /* No private error forwarding. */ }
      const code = response.status === 404 ? -32060 : [401,403].includes(response.status) ? -32061 :
        response.status === 429 ? (body?.code === 'prover_daily_budget_exhausted' ? -32062 : -32063) :
        response.status === 503 ? -32068 : -32069;
      const error = new RelayError(code);
      // Definite pre-dispatch refusals need not leave the single slot unresolved.
      error.refused = [400,401,403,404,413,429].includes(response.status) ||
        (response.status === 503 && body?.code === 'prover_queue_full');
      throw error;
    }
    return jsonBounded(response);
  }
  async prove(params) {
    validateInput(params);
    const digest = sha(JSON.stringify(canonical(params)));
    if (this.active.has(digest)) return this.active.get(digest);
    const row = this.reserve(digest);
    const promise = this.follow(row, params).finally(() => this.active.delete(digest));
    this.active.set(digest, promise); return promise;
  }
  async follow(row, params) {
    if (row.state === 'refused') fail(JSON.parse(row.reply).code);
    const deadline = this.now() + 900_000;
    let reply = row.reply ? JSON.parse(row.reply) : null;
    if (!reply) {
      if (this.status().retry_after_seconds) fail(-32063);
      try {
        reply = await this.http(ROUTE, { method: 'POST', headers: { 'Idempotency-Key': row.idem }, body: JSON.stringify(params) });
      } catch (e) {
        if (e.refused) this.db.prepare("UPDATE jobs SET state='refused',reply=? WHERE key_hash=? AND digest=?")
          .run(JSON.stringify({ code: e.code }), this.keyHash, row.digest);
        throw e;
      }
      reply = this.save(row.digest, reply);
    }
    while (!reply.terminal) {
      const seconds = Number.isFinite(reply.pollAfterSeconds) && reply.pollAfterSeconds > 0 ? reply.pollAfterSeconds : 10;
      const delay = Math.max(seconds * 1000, this.status().retry_after_seconds * 1000);
      if (this.now() + delay >= deadline) fail(-32065, reply.jobId);
      await this.wait(delay);
      reply = await this.http(ROUTE + '/' + reply.jobId, { method: 'GET' });
      reply = this.save(row.digest, reply);
    }
    if (reply.status === 'unknown_delivery') fail(-32064, reply.jobId);
    if (reply.status === 'unavailable') fail(-32068, reply.jobId);
    if (reply.status === 'failed') fail(-32067, reply.jobId);
    const result = reply.result;
    if (!result || typeof result.proof !== 'string' || !result.proof ||
        !Array.isArray(result.proof_facts) || !result.proof_facts.length || !result.proof_facts.every(v => typeof v === 'string') ||
        !Array.isArray(result.l2_to_l1_messages)) fail(-32069, reply.jobId);
    freshScreening(result, this.now());
    return result;
  }
  async resume() {
    // Poll known jobs after a restart, without retaining private request calldata.
    const rows = this.db.prepare("SELECT * FROM jobs WHERE key_hash=? AND job IS NOT NULL AND state IN ('queued','dispatched')").all(this.keyHash);
    for (const row of rows) {
      if (this.active.has(row.digest)) continue;
      const promise = this.follow(row).finally(() => this.active.delete(row.digest));
      this.active.set(row.digest, promise);
      void promise.catch(() => {});
    }
  }
  async rpc(body) {
    const id = typeof body?.id === 'string' || Number.isFinite(body?.id) ? body.id : null;
    try {
      if (!body || body.jsonrpc !== '2.0' || id === null) fail(-32602);
      let result;
      if (body.method === 'starknet_specVersion') result = '0.10.0';
      else if (body.method === 'starknet_chainId') result = MAINNET;
      else if (body.method === 'gravity_relayStatus') result = this.status();
      else if (body.method === 'starknet_proveTransaction') result = await this.prove(body.params);
      else return { jsonrpc: '2.0', id, error: { code: -32601, message: 'Method not found' } };
      return { jsonrpc: '2.0', id, result };
    } catch (e) {
      const safe = e instanceof RelayError ? e : new RelayError(-32069);
      return { jsonrpc: '2.0', id, error: { code: safe.code, message: safe.message, ...(safe.jobId ? { data: { job_id: safe.jobId } } : {}) } };
    }
  }
}

export function allowedRequest(req) {
  return !req.headers.origin && !req.headers['sec-fetch-site'] &&
    [`127.0.0.1:${PORT}`, `localhost:${PORT}`].includes(req.headers.host) &&
    req.url === '/' && req.method === 'POST' &&
    /^application\/json(?:\s*;\s*charset=utf-8)?$/i.test(req.headers['content-type'] ?? '');
}
export function makeServer(relay) {
  return createServer({ requestTimeout: 30_000, headersTimeout: 10_000 }, async (req, res) => {
    res.setHeader('Content-Type', 'application/json'); res.setHeader('Cache-Control', 'no-store');
    if (!allowedRequest(req)) { res.writeHead(403); res.end('{}'); return; }
    try {
      let size = 0; const parts = [];
      for await (const part of req) { size += part.length; if (size > MAX_INPUT) { res.writeHead(413); res.end('{}'); return; } parts.push(part); }
      const value = JSON.parse(Buffer.concat(parts).toString('utf8'));
      const result = await relay.rpc(value);
      res.end(JSON.stringify(result));
    } catch { res.writeHead(400); res.end('{}'); }
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const apiKey = (process.env.STARKSCAN_API_KEY ?? readFileSync(join(homedir(), '.config/starkscan/api_key'), 'utf8')).trim();
    const directory = join(homedir(), '.local/share/gravity/starkscan-relay');
    const relay = new StarkscanRelay({ apiKey, directory });
    const server = makeServer(relay);
    server.on('error', () => { console.error('Cannot listen on 127.0.0.1:3001. No new relay started.'); relay.close(); process.exitCode = 1; });
    server.listen(PORT, '127.0.0.1', () => {
      console.log('gravity screened-deposit adapter: http://127.0.0.1:3001 (upstream proof not yet verified)');
      void relay.resume();
    });
  } catch { console.error('Cannot start relay. Check Node 24+, private credential and storage permissions.'); process.exitCode = 1; }
}
