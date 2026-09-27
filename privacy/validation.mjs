export const STRK = '0x04718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d';
export class PrivacyError extends Error {
  constructor(code, message) { super(message); this.code = code; }
}
export const fail = (code, message) => { throw new PrivacyError(code, message); };
export function felt(value) {
  if (typeof value !== 'string' || !/^(0x[0-9a-fA-F]+|[0-9]+)$/.test(value)) fail('INPUT', 'Invalid field value');
  const n = BigInt(value);
  if (n < 0n || n >= (1n << 251n) + 17n * (1n << 192n) + 1n) fail('INPUT', 'Field value out of range');
  return '0x' + n.toString(16);
}
export function amount(value) {
  if (typeof value !== 'string' || !/^[0-9]+$/.test(value)) fail('INPUT', 'Amount must be whole base units');
  const n = BigInt(value);
  if (n <= 0n || n >= 1n << 256n) fail('INPUT', 'Amount must be positive and fit u256');
  return n;
}
export function endpoint(value) {
  let u;
  try { u = new URL(value); } catch { fail('CONFIG', 'Configure a valid service endpoint'); }
  if (u.username || u.password || u.hash || !(
    u.protocol === 'https:' || (u.protocol === 'http:' && ['127.0.0.1', '[::1]', 'localhost'].includes(u.hostname)))) {
    fail('CONFIG', 'Use HTTPS or an SSH tunnel on loopback for privacy services');
  }
  return u;
}
export function screeningRequired(operation, proof, policy = 'required') {
  if (!['required','pool_enforced'].includes(policy)) fail('CONFIG','Unknown screening policy');
  if (operation === 'deposit' && policy === 'required' && !proof.additionalData?.signature) {
    fail('SCREENING_REQUIRED', 'Your prover returned no deposit-screening signature. Configure its screening integration before shielding. No transaction was submitted.');
  }
}
export function approvalCalls(pool, fee, operation, token, quantity) {
  const amounts = new Map([[felt(STRK), BigInt(fee)]]);
  if (operation === 'deposit') amounts.set(felt(token), (amounts.get(felt(token)) ?? 0n) + quantity);
  return [...amounts].filter(([, n]) => n > 0n).map(([contractAddress, n]) => ({
    contract_address: contractAddress, entrypoint:'approve',
    calldata:[felt(pool), felt((n & ((1n<<128n)-1n)).toString()), felt((n>>128n).toString())],
  }));
}
