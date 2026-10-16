// Adapt strkd 21daa96 to Gravity's pinned SDK/indexer. A note spent or a
// channel advanced after the proof base would waste a proof and revert on-chain.
import { AddressMap, IndexerDiscoveryProvider } from '@starkware-libs/starknet-privacy-sdk';
import { fail } from './validation.mjs';

// Compare semantic state without cursor/timestamp differences or service order.
// Includes channel witnesses/nonces and complete note contents; never log it.
function canonical(value) {
  if (typeof value === 'bigint') return value.toString();
  if (value instanceof Map || value instanceof AddressMap) {
    return [...value].map(([k, v]) => [canonical(k), canonical(v)])
      .sort((a, b) => JSON.stringify(a[0]).localeCompare(JSON.stringify(b[0])));
  }
  if (Array.isArray(value)) return value.map(canonical)
    .sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
  if (value && typeof value === 'object') return Object.fromEntries(
    Object.keys(value).sort().map(k => [k, canonical(value[k])]));
  return value;
}

// Incoming transfers may add notes after the proof base. Those notes are not
// spend inputs and must not block spending the unchanged mature snapshot.
// Consume a multiset of complete note payloads per token so missing, changed,
// or duplicated mature inputs still fail closed.
function matureNotesUnchanged(mature, current) {
  for (const [token, notes] of mature.notes) {
    const available = new Map();
    for (const note of current.notes.get(token) ?? []) {
      const key = JSON.stringify(canonical(note));
      available.set(key, (available.get(key) ?? 0) + 1);
    }
    for (const note of notes) {
      const key = JSON.stringify(canonical(note));
      const count = available.get(key) ?? 0;
      if (!count) return false;
      available.set(key, count - 1);
    }
  }
  return true;
}

export class MatureDiscoveryProvider extends IndexerDiscoveryProvider {
  constructor(url, pool, base, currentHead) {
    super(url, pool);
    this.base = base;
    this.currentHead = currentHead;
  }

  async compare(read, unchanged) {
    const head = await this.currentHead();
    if (!Number.isSafeInteger(head) || head < this.base) fail('STATE', 'Invalid discovery comparison block');
    const mature = await read(this.base);
    const current = await read(head);
    if (mature.timestamp !== this.base || current.timestamp !== head) {
      fail('DISCOVERY', 'Discovery did not return the requested block');
    }
    if (!unchanged(mature, current)) {
      fail('STALE_STATE', 'Private notes or channels changed after the proof base. Wait for the recent transaction to mature, then try again.');
    }
    return mature;
  }

  async discoverNotes(address, viewingKey, params = {}) {
    if (params.blockIdentifier !== this.base || params.cursor) fail('STATE', 'Fresh discovery at the proof base is required');
    return this.compare(blockIdentifier => super.discoverNotes(address, viewingKey, { ...params, blockIdentifier }), matureNotesUnchanged);
  }

  async discoverChannels(address, viewingKey, recipients, params = {}) {
    if (params.blockIdentifier !== this.base || params.cursor) fail('STATE', 'Fresh discovery at the proof base is required');
    const state = result => JSON.stringify(canonical({ channels: result.channels, total: result.total }));
    return this.compare(blockIdentifier => super.discoverChannels(address, viewingKey, recipients, { ...params, blockIdentifier }),
      (mature, current) => state(mature) === state(current));
  }
}
