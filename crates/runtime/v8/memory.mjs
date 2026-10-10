import { compatible, eip } from './state.mjs';
import { Mappings } from './mappings.mjs';

const ownedMemories = new WeakSet();

function* pages(start, bytes) {
  const count = bytes === 0 ? 0 : Math.floor(((start & 4095) + bytes - 1) / 4096) + 1;
  for (let i = 0; i < count; i++) yield ((start >>> 12) + i) & 0xfffff;
}
function addDependency(map, key, id) {
  if (!map.has(key)) map.set(key, new Set());
  map.get(key).add(id);
}
function removeDependency(map, key, id) {
  const tickets = map.get(key);
  tickets.delete(id);
  if (tickets.size !== 0) return false;
  map.delete(key);
  return true;
}

/** Execution-thread memory owner. Writes and remaps invalidate pending and
 * installed tickets before publishing mutations; workers never see live memory.
 */
export class HostMemory {
  #cpu;
  #guest;
  #table;
  #profile;
  #mappings;
  #context;
  #nextId = 1;
  #entries = new Map();
  #installed = new Map();
  #pending = new Map();
  #byMapping = new Map();
  #byBacking = new Map();
  #executing = false;
  #closed = false;

  constructor(imports, profile) {
    this.#cpu = imports.cpuState;
    this.#guest = imports.guest;
    this.#table = profile === 'real16' ? imports.physicalMap : imports.machine;
    this.#profile = profile;
    for (const memory of [this.#cpu, this.#guest, this.#table]) {
      if (!(memory instanceof WebAssembly.Memory) || memory.buffer instanceof SharedArrayBuffer) {
        throw new TypeError('private CPU, backing and mapping memories are required');
      }
    }
    for (const memory of [this.#cpu, this.#guest, this.#table]) {
      if (ownedMemories.has(memory)) throw new Error('guest memories already have an execution owner');
    }
    this.#mappings = new Mappings(this.#table, profile === 'real16');
    for (const memory of [this.#cpu, this.#guest, this.#table]) ownedMemories.add(memory);
  }

  #checkOpen() {
    if (this.#closed) throw new Error('memory owner is closed');
  }

  assertBoundary() {
    this.#checkOpen();
    if (this.#executing) throw new Error('operation requires an execution boundary');
  }

  close() {
    if (this.#closed) return;
    this.assertBoundary();
    this.clear();
    this.#closed = true;
    for (const memory of [this.#cpu, this.#guest, this.#table]) ownedMemories.delete(memory);
  }

  enter() {
    const view = new DataView(this.#cpu.buffer);
    const base = view.getUint32(76, true);
    const limit = view.getUint32(80, true);
    const attributes = view.getUint32(84, true); // selector and attributes
    const valid = compatible(this.#cpu, this.#profile);
    if (!valid || this.#context?.base !== base || this.#context?.limit !== limit || this.#context?.attributes !== attributes) {
      this.clear();
      this.#context = valid ? { base, limit, attributes } : undefined;
    }
    return valid;
  }

  lookup() { return this.#installed.get(eip(this.#cpu)); }
  contains(id) { return this.#entries.has(id); }

  isPending(id) {
    const entry = this.#entries.get(id);
    return entry !== undefined && this.#pending.get(entry.address) === id;
  }

  #codeContext() {
    if (!this.enter()) return null;
    return [3, 7].includes(this.#context.attributes >>> 16 & 15) ? this.#context : null;
  }

  // Resolve the mapped prefix once for both capture and complete registration.
  #fetchSpans(cs, offset, bytes) {
    const spans = [];
    let remaining = bytes;
    while (remaining > 0 && offset <= cs.limit) {
      const linear = (cs.base + offset) >>> 0;
      const mapping = this.#mappings.get(linear >>> 12);
      if (mapping.kind !== 'ram') break;
      const count = Math.min(remaining, 4096 - (linear & 4095), cs.limit - offset + 1);
      spans.push({ page: linear >>> 12, backing: mapping.backing,
        start: mapping.backing + (linear & 4095), bytes: count });
      offset = (offset + count) >>> 0;
      remaining -= count;
      if (cs.limit !== 0xffffffff && offset === 0) break;
    }
    return spans;
  }

  #protect(address, spans) {
    if (!Number.isSafeInteger(this.#nextId)) throw new RangeError('code ticket overflow');
    const mappings = new Set(spans.map(span => span.page));
    const backing = new Set(spans.map(span => span.backing));
    this.cancel(this.#pending.get(address));
    const id = this.#nextId++;
    for (const page of mappings) addDependency(this.#byMapping, page, id);
    for (const frame of backing) {
      const first = !this.#byBacking.has(frame);
      addDependency(this.#byBacking, frame, id);
      if (first) this.#mappings.watch(frame, true);
    }
    this.#entries.set(id, { address, mappings, backing });
    this.#pending.set(address, id);
    return id;
  }

  /** Protect complete CS-relative ranges without copying or decoding guest bytes.
   * The loader establishes artifact identity before registering its dependencies.
   */
  register(address, ranges) {
    this.assertBoundary();
    const u32 = value => Number.isInteger(value) && value >= 0 && value <= 0xffffffff;
    if (!u32(address) || ranges.some(range => !u32(range.offset) || !u32(range.bytes))) {
      throw new RangeError('invalid code range');
    }
    if (ranges.some(range => range.bytes === 0)
      || !ranges.some(range => ((address - range.offset) >>> 0) < range.bytes)) return null;
    const cs = this.#codeContext();
    if (!cs) return null;
    const spans = [];
    for (const range of ranges) {
      const fetched = this.#fetchSpans(cs, range.offset, range.bytes);
      if (fetched.reduce((bytes, span) => bytes + span.bytes, 0) !== range.bytes) return null;
      for (const span of fetched) spans.push(span);
    }
    return this.#protect(address, spans);
  }

  capture(address, instructionLimit) {
    this.assertBoundary();
    const cs = this.#codeContext();
    if (!cs) return null;
    const spans = this.#fetchSpans(cs, address, 15 * instructionLimit);
    if (spans.length === 0) return null;
    const id = this.#protect(address, spans);
    // Reserve, copy and enqueue have no asynchronous gap or guest invocation.
    const code = [];
    for (const span of spans) code.push(...new Uint8Array(this.#guest.buffer, span.start, span.bytes));
    return { id, request: { profile: this.#profile, kind: 'block', eip: address, code, instruction_limit: instructionLimit } };
  }

  install(id) {
    this.assertBoundary();
    const entry = this.#entries.get(id);
    if (!entry || this.#pending.get(entry.address) !== id) return false;
    this.cancel(this.#installed.get(entry.address));
    this.#pending.delete(entry.address);
    this.#installed.set(entry.address, id);
    return true;
  }

  cancel(id) {
    const entry = this.#entries.get(id);
    if (!entry) return;
    this.#entries.delete(id);
    if (this.#installed.get(entry.address) === id) this.#installed.delete(entry.address);
    if (this.#pending.get(entry.address) === id) this.#pending.delete(entry.address);
    for (const page of entry.mappings) removeDependency(this.#byMapping, page, id);
    for (const frame of entry.backing) {
      if (removeDependency(this.#byBacking, frame, id)) this.#mappings.watch(frame, false);
    }
  }

  clear() {
    for (const backing of this.#byBacking.keys()) this.#mappings.watch(backing, false);
    this.#entries.clear();
    this.#installed.clear();
    this.#pending.clear();
    this.#byMapping.clear();
    this.#byBacking.clear();
  }

  #invalidateBacking(start, bytes) {
    for (const page of pages(start, bytes)) {
      for (const id of [...(this.#byBacking.get(page * 4096) ?? [])]) this.cancel(id);
    }
  }

  invalidateWrite(start, bytes) {
    for (const page of pages(start, bytes)) {
      const mapping = this.#mappings.get(page);
      if (mapping.kind === 'ram') this.#invalidateBacking(mapping.backing, 4096);
    }
  }

  writeBacking(offset, bytes) {
    this.#checkOpen();
    if (!Number.isInteger(offset) || offset < 0 || offset > 0xffffffff) throw new RangeError('invalid backing offset');
    // Own the input before invalidation: it may be a view of this guest's RAM.
    const value = Uint8Array.from(bytes);
    if (offset + value.length > this.#guest.buffer.byteLength) throw new RangeError('backing write outside memory');
    this.#invalidateBacking(offset, value.length);
    new Uint8Array(this.#guest.buffer, offset, value.length).set(value);
  }

  remap(page, mapping) {
    this.#checkOpen();
    this.#mappings.validate(page, mapping);
    for (const id of [...(this.#byMapping.get(page) ?? [])]) this.cancel(id);
    this.#mappings.replace(page, mapping);
    this.#mappings.write(page, mapping.kind === 'ram' && this.#byBacking.has(mapping.backing));
  }

  invoke(entry) {
    this.assertBoundary();
    this.#executing = true;
    try { return entry(); }
    finally { this.#executing = false; }
  }
}
