import assert from 'node:assert/strict';
import test from 'node:test';
import { Worker } from 'node:worker_threads';
import { once } from 'node:events';
import { Runtime } from './runtime.mjs';
import { cpuMemory, waitFor } from './test-support.mjs';

const generator = new URL('../../../target/wasm32-unknown-unknown/release/wasm86_codegen.wasm', import.meta.url);

function machine(t, code, profile = 'flat32') {
  const cpuState = cpuMemory();
  const guest = new WebAssembly.Memory({ initial: 1 });
  const mapping = new WebAssembly.Memory({ initial: 64 });
  const cpu = new DataView(cpuState.buffer);
  new Uint8Array(guest.buffer).set(code, 0x1000);
  for (let page = 1; page < 16; page++) new DataView(mapping.buffer).setUint32(page * 4, (page << 12) | 3, true);
  const runtime = new Runtime({ generator, profile, imports: { wasm86: {
    cpuState, guest, machine: mapping,
    resolveSegment() { throw new Error('unexpected segment resolution'); },
    querySegmentDescriptor() { throw new Error('unexpected segment query'); },
  } } });
  t.after(() => runtime.close());
  return { runtime, cpu, guest, mapping };
}

test('generation and compilation stay off the execution thread while guest slices continue', { timeout: 120000 }, async t => {
  const originalCompile = WebAssembly.compile;
  const originalModule = WebAssembly.Module;
  const originalInstance = WebAssembly.Instance;
  let blockCalls = 0;
  WebAssembly.compile = () => { throw new Error('compilation on execution thread'); };
  WebAssembly.Module = new Proxy(originalModule, { construct() { throw new Error('synchronous compilation'); } });
  WebAssembly.Instance = new Proxy(originalInstance, {
    construct(target, args) {
      assert(!originalModule.exports(args[0]).some(entry => entry.name === 'generate'), 'generator must be instantiated on worker');
      const instance = Reflect.construct(target, args);
      // Wrap only test-visible exports, without changing guest effects.
      const exports = Object.fromEntries(Object.entries(instance.exports).map(([name, entry]) => [
        name, name.startsWith('block_') ? (...args) => { blockCalls++; return entry(...args); } : entry,
      ]));
      return { exports };
    },
  });
  t.after(() => {
    WebAssembly.compile = originalCompile;
    WebAssembly.Module = originalModule;
    WebAssembly.Instance = originalInstance;
  });
  const code = [0x40, 0xeb, 0xfd];
  const { runtime, cpu } = machine(t, code);
  let heartbeats = 0;
  const heartbeat = setInterval(() => heartbeats++, 1);
  t.after(() => clearInterval(heartbeat));
  assert.equal((await runtime.runSlice(10)).exit, 'starting');
  assert.equal((await waitFor(runtime, 0)).kind, 'installed');
  assert(heartbeats > 0, 'host event loop runs during interpreter generation');
  const id = runtime.requestBlock(0x1000, 2);
  assert.equal((await waitFor(runtime, id, 10)).kind, 'installed');
  const before = cpu.getUint32(28, true);
  assert(before > 0, 'guest made progress during compilation/installation');
  assert.equal((await runtime.runSlice(10)).exit, 'yielded');
  assert.equal(cpu.getUint32(28, true), before + 5);
  assert(blockCalls > 0, 'dispatch must enter installed compiled code');
  // Snapshots are copied and invalidation rejects late completions.
  const stale = runtime.requestBlock(0x1000, 2);
  runtime.memory.writeBacking(0x1000, [0x48]);
  const callsBeforeInvalidation = blockCalls;
  assert.equal((await waitFor(runtime, stale, 10)).kind, 'discarded');
  assert.equal(blockCalls, callsBeforeInvalidation, 'invalidated code must not execute');
});

test('protected stores, host writes and remaps invalidate installed code', { timeout: 120000 }, async t => {
  // MOV byte [0x1007], 0x48; INC EAX; JMP -3.
  const code = [0xc6, 0x05, 0x07, 0x10, 0, 0, 0x48, 0x40, 0xeb, 0xfd];
  const { runtime, cpu } = machine(t, code);
  assert.equal((await waitFor(runtime, 0)).kind, 'installed');
  const id = runtime.requestBlock(0x1000, 3);
  assert.equal((await waitFor(runtime, id)).kind, 'installed');
  await runtime.runSlice(2);
  assert.equal(cpu.getUint32(28, true), 0xffffffff);
  assert.equal(cpu.getUint32(144, true), 2);
  const next = runtime.requestBlock(0x1007, 2);
  assert.equal((await waitFor(runtime, next)).kind, 'installed');
  runtime.memory.writeBacking(0x1007, [0x40]);
  cpu.setUint32(60, 0x1007, true);
  await runtime.runSlice(1);
  assert.equal(cpu.getUint32(28, true), 0);
  cpu.setUint32(60, 0x1007, true);
  const unmapped = runtime.requestBlock(0x1007, 1);
  assert.equal((await waitFor(runtime, unmapped)).kind, 'installed');
  runtime.memory.remap(1, { kind: 'unmapped' });
  const fault = await runtime.runSlice(1);
  assert.equal(fault.exit, 'guest');
  assert.equal(fault.value, (4n << 48n) | (16n << 32n) | 0x1007n);
  assert.equal(cpu.getUint32(144, true), 3);
});

test('REP resumes across installation and a failed compilation leaves interpretation available', { timeout: 120000 }, async t => {
  const { runtime, cpu, guest } = machine(t, [0xf3, 0xaa]);
  assert.equal((await waitFor(runtime, 0)).kind, 'installed');
  cpu.setUint32(28, 0x5a, true);
  cpu.setUint32(32, 100, true);
  cpu.setUint32(56, 0x2000, true);
  await runtime.runSlice(2);
  assert.equal(cpu.getUint32(32, true), 98);
  assert.equal(cpu.getUint32(144, true), 0);
  const id = runtime.requestBlock(0x1000, 1);
  assert.equal((await waitFor(runtime, id)).kind, 'installed');
  for (let i = 0; i < 49; i++) assert.equal((await runtime.runSlice(2)).exit, 'yielded');
  assert.equal(cpu.getUint32(60, true), 0x1002);
  assert.equal(cpu.getUint32(144, true), 1);
  assert.deepEqual(new Uint8Array(guest.buffer, 0x2000, 100), new Uint8Array(100).fill(0x5a));
  runtime.memory.writeBacking(0x1002, [0x0f, 0xff]);
  const failed = runtime.requestBlock(0x1002, 1);
  assert.equal((await waitFor(runtime, failed)).kind, 'failed');
  runtime.memory.writeBacking(0x1002, [0x40]);
  assert.equal((await runtime.runSlice(1)).exit, 'yielded');
  assert.equal(cpu.getUint32(28, true), 0x5b);
});

test('queue bounds, superseded work, profile rejection, and shutdown', { timeout: 120000 }, async t => {
  const { runtime, cpu } = machine(t, [0x40, 0xeb, 0xfd]);
  assert.equal((await waitFor(runtime, 0)).kind, 'installed');
  const tickets = [];
  for (let i = 0; i < 8; i++) tickets.push(runtime.requestBlock(0x1000, 2));
  assert.equal(runtime.requestBlock(0x1000, 1), null);
  // Poll all completions together so no event is lost when several arrive at once.
  const events = new Map();
  const deadline = Date.now() + 60000;
  while (events.size < 8 && Date.now() < deadline) {
    for (const event of (await runtime.runSlice(0)).compilations) events.set(event.id, event.kind);
  }
  assert.equal(events.size, 8);
  assert.equal(events.get(tickets.at(-1)), 'installed');
  for (const id of tickets.slice(0, -1)) assert.equal(events.get(id), 'discarded');
  const active = runtime.runSlice(0);
  await assert.rejects(runtime.runSlice(0), /already active/);
  await active;
  cpu.setUint16(86, 7, true); // CS defaults no longer match flat32.
  assert.equal((await runtime.runSlice(1)).exit, 'incompatible_profile');
  await runtime.close();
  await assert.rejects(runtime.runSlice(1), /closed/);
});

test('pending snapshots reject mapping ABA and changed CS context', { timeout: 120000 }, async t => {
  const { runtime, cpu, mapping } = machine(t, [0x40, 0xeb, 0xfd]);
  assert.equal((await waitFor(runtime, 0)).kind, 'installed');
  runtime.memory.remap(3, { kind: 'ram', backing: 0x1000, writable: true });
  const stale = runtime.requestBlock(0x1000, 2);
  assert(new DataView(mapping.buffer).getUint32(12, true) & 0x10);
  runtime.memory.remap(1, { kind: 'ram', backing: 0x4000, writable: true });
  runtime.memory.remap(1, { kind: 'ram', backing: 0x1000, writable: true });
  assert.equal((await waitFor(runtime, stale)).kind, 'discarded');
  assert.equal(new DataView(mapping.buffer).getUint32(12, true) & 0x10, 0);
  const changed = runtime.requestBlock(0x1000, 2);
  cpu.setUint16(84, 8, true);
  assert.equal((await waitFor(runtime, changed)).kind, 'discarded');
  assert.equal((await runtime.runSlice(2)).exit, 'yielded');
  assert.equal(cpu.getUint32(28, true), 1);
});

test('REP writing its prefix redecodes after one checked element', { timeout: 120000 }, async t => {
  const { runtime, cpu } = machine(t, [0xf3, 0xaa, 0x40]);
  assert.equal((await waitFor(runtime, 0)).kind, 'installed');
  cpu.setUint32(28, 0x90, true);
  cpu.setUint32(32, 3, true);
  cpu.setUint32(56, 0x1000, true);
  const id = runtime.requestBlock(0x1000, 1);
  assert.equal((await waitFor(runtime, id)).kind, 'installed');
  assert.equal((await runtime.runSlice(1)).exit, 'yielded');
  assert.equal(cpu.getUint32(60, true), 0x1000);
  assert.equal(cpu.getUint32(32, true), 2);
  assert.equal(cpu.getUint32(56, true), 0x1001);
  assert.equal(cpu.getUint32(144, true), 0);
  assert.equal((await runtime.runSlice(1)).exit, 'yielded');
  assert.equal(cpu.getUint32(60, true), 0x1001);
  assert.equal(cpu.getUint32(32, true), 2);
  assert.equal(cpu.getUint32(56, true), 0x1001);
  assert.equal(cpu.getUint32(144, true), 1);
});

test('prepared module installation is independent of jobs and obeys invalidation', { timeout: 120000 }, async t => {
  const producer = new Worker(new URL('./compiler-worker.mjs', import.meta.url), {
    workerData: { generator: generator.href },
  });
  t.after(() => producer.terminate());
  const ready = once(producer, 'message');
  producer.postMessage({ id: 1, request: {
    profile: 'flat32', kind: 'block', eip: 0x1000, code: [0x40], instruction_limit: 1,
  } });
  const [prepared] = await ready;
  assert.equal(prepared.error, undefined);
  await producer.terminate();
  const { runtime, cpu, mapping } = machine(t, [0x40, 0xeb, 0xfd]);
  assert.equal((await waitFor(runtime, 0)).kind, 'installed');
  const originalInstance = WebAssembly.Instance;
  let calls = 0;
  WebAssembly.Instance = new Proxy(originalInstance, {
    construct(target, args) {
      const instance = Reflect.construct(target, args);
      if (args[0] !== prepared.module) return instance;
      return { exports: { [prepared.entry]: () => { calls++; return instance.exports[prepared.entry](); } } };
    },
  });
  t.after(() => { WebAssembly.Instance = originalInstance; });
  runtime.memory.remap(3, { kind: 'ram', backing: 0x1000, writable: true });
  runtime.memory.writeBacking(0x2000, [0xc6, 0x05, 0, 0x30, 0, 0, 0x48]);
  const ranges = [{ offset: 0x1000, bytes: 1 }];
  const ticket = runtime.registerCode(0x1000, ranges);
  assert.notEqual(ticket, null);
  assert(runtime.install(ticket, prepared));
  assert(!runtime.install(ticket, prepared));
  const failed = runtime.registerCode(0x1000, ranges);
  assert.throws(() => runtime.install(failed, { ...prepared, entry: 'missing' }), /no entry function/);
  assert(!runtime.install(failed, prepared));
  assert(new DataView(mapping.buffer).getUint32(12, true) & 0x10);
  const slice = await runtime.runSlice(2);
  assert.equal(slice.exit, 'yielded');
  assert.deepEqual(slice.compilations, []);
  assert.equal(cpu.getUint32(28, true), 1);
  assert.equal(calls, 1, 'failed replacement preserves the independently prepared module');

  cpu.setUint32(60, 0x2000, true);
  assert.equal((await runtime.runSlice(1)).exit, 'yielded');
  assert.equal(new DataView(mapping.buffer).getUint32(4, true) & 0x10, 0);
  assert(!runtime.install(ticket, prepared));
  cpu.setUint32(60, 0x1000, true);
  assert.equal((await runtime.runSlice(2)).exit, 'yielded');
  assert.equal(cpu.getUint32(28, true), 0);
  assert.equal(calls, 1, 'the alias write removes the prepared entry');

  runtime.memory.writeBacking(0x1000, [0x40]);
  const stale = runtime.registerCode(0x1000, ranges);
  runtime.memory.remap(1, { kind: 'ram', backing: 0x4000, writable: true });
  runtime.memory.remap(1, { kind: 'ram', backing: 0x1000, writable: true });
  assert(!runtime.install(stale, prepared));
  const abandoned = runtime.registerCode(0x1000, ranges);
  runtime.cancelCode(abandoned);
  assert.equal(new DataView(mapping.buffer).getUint32(12, true) & 0x10, 0);
  // Install against a JIT ticket before its worker stops. The code lifetime
  // survives disposal of the now-unneeded compilation request.
  const originalPost = Worker.prototype.postMessage;
  let stopped;
  Worker.prototype.postMessage = function () { stopped = this.terminate(); };
  let next;
  try { next = runtime.requestBlock(0x1000, 1); }
  finally { Worker.prototype.postMessage = originalPost; }
  assert(runtime.install(next, prepared));
  await stopped;
  const afterStop = await runtime.runSlice(2);
  assert.equal(afterStop.exit, 'yielded');
  assert.deepEqual(afterStop.compilations, [{ kind: 'discarded', id: next }]);
  assert.equal(cpu.getUint32(28, true), 1);
  assert.equal(calls, 2, 'worker failure does not cancel independently installed code');
  runtime.memory.remap(1, { kind: 'unmapped' });
  const fault = await runtime.runSlice(1);
  assert.equal(fault.exit, 'guest');
  assert.equal(fault.value, (4n << 48n) | (16n << 32n) | 0x1000n);
  assert.equal(cpu.getUint32(144, true), 7);
});
