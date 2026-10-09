import { Worker } from 'node:worker_threads';
import { setImmediate } from 'node:timers/promises';
import { HostMemory } from './memory.mjs';

const EXHAUSTED = 512n << 48n;
const DISPATCH = 1024n << 48n;
const INTERPRET = 2048n << 48n;
const MAX_PENDING = 8;

/** One CPU execution owner and one off-thread Rust generator / V8 compiler.
 * The host supplies the wasm86 memory and device imports except executionBudget,
 * dispatch, interpret and invalidation. No live memory is passed to the compilation worker.
 */
export class Runtime {
  #worker;
  #imports;
  #budget = new WebAssembly.Memory({ initial: 1 });
  #pending = new Set();
  #memory;
  #blocks = new Map();
  #completed = [];
  #interpreter;
  #interpretNext = false;
  #failure;
  #startupFailed = false;
  #closed = false;
  #running = false;
  #closing;

  constructor({ generator, imports, profile = 'flat32' }) {
    if (!['flat32', 'segmented32', 'segmented16', 'real16'].includes(profile)) {
      throw new TypeError('unknown execution profile');
    }
    if (!(imports?.wasm86?.cpuState instanceof WebAssembly.Memory)) {
      throw new TypeError('cpuState memory is required');
    }
    this.#memory = new HostMemory(imports.wasm86, profile);
    this.#imports = { ...imports, wasm86: { ...imports.wasm86,
      executionBudget: this.#budget, dispatch: () => DISPATCH, interpret: () => INTERPRET,
      invalidateCode: (address, bytes) => this.#memory.invalidateWrite(address >>> 0, bytes >>> 0),
    } };
    try {
      this.#worker = new Worker(new URL('./compiler-worker.mjs', import.meta.url), {
        workerData: { generator: new URL(generator).href },
      });
      this.#worker.on('message', completion => this.#completed.push(completion));
      this.#worker.on('error', error => { this.#failure = String(error); });
      this.#worker.on('exit', code => {
        if (!this.#closed) this.#failure ??= `compilation worker exited (${code})`;
      });
      this.#pending.add(0);
      this.#worker.postMessage({ id: 0, request: { profile, kind: 'interpreter' } });
    } catch (error) {
      this.#memory.close();
      void this.#worker?.terminate();
      throw error;
    }
  }

  get memory() { return this.#memory; }

  /** Protect and copy live fetch bytes. Returns null when the bounded queue is full. */
  requestBlock(eip, instructionLimit) {
    if (this.#closed || this.#failure) throw new Error(this.#failure ?? 'runtime is closed');
    this.#memory.assertBoundary();
    if (this.#pending.size >= MAX_PENDING) return null;
    if (!Number.isInteger(eip) || eip < 0 || eip > 0xffffffff) throw new RangeError('invalid EIP');
    if (!Number.isInteger(instructionLimit) || instructionLimit < 1 || instructionLimit > 256) throw new RangeError('instruction limit is outside 1..=256');
    const job = this.#memory.capture(eip, instructionLimit);
    if (!job) throw new Error('no readable code under the current execution context');
    try { this.#worker.postMessage(job); }
    catch (error) { this.#memory.cancel(job.id); throw error; }
    this.#pending.add(job.id);
    return job.id;
  }

  /** Register the complete code dependencies of a prepared artifact. */
  registerCode(eip, ranges) { return this.#memory.register(eip, ranges); }

  cancelCode(ticket) {
    this.#memory.cancel(ticket);
    this.#blocks.delete(ticket);
  }

  /** Install an engine-compiled module using the generated ABI/profile, with no
   * start function or imported-memory initialization. The loader matches its
   * code to the registered guest image.
   * Returns false for stale/already installed tickets; failure releases watches.
   */
  install(ticket, { module, entry }) {
    this.#memory.assertBoundary();
    this.#memory.enter();
    if (!this.#memory.isPending(ticket)) return false;
    let exported;
    try {
      const instance = new WebAssembly.Instance(module, this.#imports);
      exported = instance.exports[entry];
      if (typeof exported !== 'function') throw new TypeError('module has no entry function');
    } catch (error) { this.cancelCode(ticket); throw error; }
    if (!this.#memory.install(ticket)) throw new Error('live installation ticket was lost');
    this.#blocks.set(ticket, exported);
    return true;
  }

  #installReady() {
    this.#memory.enter();
    const events = [];
    for (const completion of this.#completed.splice(0)) {
      const { id } = completion;
      if (!this.#pending.has(id)) continue;
      const ticket = id === 0 ? null : id;
      this.#pending.delete(id);
      if (ticket !== null && !this.#memory.isPending(ticket)) {
        events.push({ kind: 'discarded', id });
        continue;
      }
      try {
        if (completion.error !== undefined) {
          if (ticket !== null) this.cancelCode(ticket);
          throw new Error(completion.error);
        }
        if (ticket === null) {
          const instance = new WebAssembly.Instance(completion.module, this.#imports);
          this.#interpreter = instance.exports[completion.entry];
        } else if (!this.install(ticket, completion)) {
          events.push({ kind: 'discarded', id });
          continue;
        }
        events.push({ kind: 'installed', id });
      } catch (error) {
        if (ticket === null) this.#startupFailed = true;
        events.push({ kind: 'failed', id, error: String(error) });
      }
    }
    if (this.#failure) {
      for (const id of this.#pending) {
        const ticket = id === 0 ? null : id;
        if (ticket !== null && !this.#memory.isPending(ticket)) {
          events.push({ kind: 'discarded', id });
          continue;
        }
        if (ticket === null) this.#startupFailed = true;
        else this.#memory.cancel(ticket);
        events.push({ kind: 'failed', id, error: this.#failure });
      }
      this.#pending.clear();
    }
    for (const id of this.#blocks.keys()) if (!this.#memory.contains(id)) this.#blocks.delete(id);
    return events;
  }

  /** Yield to the host event loop, install completions, and run a bounded slice.
   * Traps reject the promise. Guest faults use the ordinary raw exit ABI.
   */
  async runSlice(work) {
    if (this.#closed) throw new Error('runtime is closed');
    if (this.#running) throw new Error('guest execution is already active');
    if (!Number.isInteger(work) || work < 0 || work > 0xffffffff) throw new RangeError('invalid work budget');
    this.#running = true;
    try {
      await setImmediate();
      if (this.#closed) throw new Error('runtime is closed');
      const compilations = this.#installReady();
      return { ...this.#execute(work), compilations };
    } finally {
      this.#running = false;
    }
  }

  #execute(work) {
    if (!this.#interpreter) return { exit: this.#startupFailed ? 'unavailable' : 'starting' };
    new DataView(this.#budget.buffer).setUint32(0, work, true);
    while (new DataView(this.#budget.buffer).getUint32(0, true) !== 0) {
      if (!this.#memory.enter()) return { exit: 'incompatible_profile' };
      const entry = this.#interpretNext ? this.#interpreter
        : this.#blocks.get(this.#memory.lookup()) ?? this.#interpreter;
      this.#interpretNext = false;
      const result = this.#memory.invoke(entry);
      if (result === INTERPRET) this.#interpretNext = true;
      else if (result === EXHAUSTED) return { exit: 'yielded' };
      else if (result !== DISPATCH) return { exit: 'guest', value: result };
    }
    return { exit: 'yielded' };
  }

  async close() {
    if (this.#closing) return this.#closing;
    this.#memory.close();
    this.#closed = true;
    this.#pending.clear();
    this.#completed.length = 0;
    this.#blocks.clear();
    this.#interpreter = undefined;
    this.#closing = this.#worker.terminate();
    return this.#closing;
  }
}
