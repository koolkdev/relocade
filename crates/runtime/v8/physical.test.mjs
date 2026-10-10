import assert from 'node:assert/strict';
import test from 'node:test';
import { Runtime } from './runtime.mjs';
import { cpuMemory, waitFor } from './test-support.mjs';

const generator = new URL('../../../target/wasm32-unknown-unknown/release/wasm86_codegen.wasm', import.meta.url);

function machine(t, code) {
  const cpuState = cpuMemory('real16');
  const guest = new WebAssembly.Memory({ initial: 1 });
  const physicalMap = new WebAssembly.Memory({ initial: 1 });
  const table = new DataView(physicalMap.buffer);
  table.setUint32(8, 2, true); // ROM code
  table.setUint32(12, 0x1000, true);
  table.setUint32(16, 3, true); // MMIO
  table.setUint32(24, 1, true); // RAM alias of the ROM
  table.setUint32(28, 0x1000, true);
  new Uint8Array(guest.buffer).set(code, 0x1000);
  let reads = 0;
  const runtime = new Runtime({ generator, profile: 'real16', imports: { wasm86: {
    cpuState, guest, physicalMap,
    readMmio(address, bytes) {
      assert.equal(address, 0x2000);
      assert.equal(bytes, 1);
      reads++;
      assert.throws(() => runtime.requestBlock(0x1000, 1), /execution boundary/);
      runtime.memory.writeBacking(0x1003, [0x48]);
      runtime.memory.remap(2, { kind: 'ram', backing: 0x2000, writable: true });
      return 0n;
    },
    writeMmio() { assert.fail('unexpected MMIO write'); },
    readPort() { assert.fail('unexpected port read'); },
    writePort() { assert.fail('unexpected port write'); },
  } } });
  t.after(() => runtime.close());
  return { runtime, cpu: new DataView(cpuState.buffer), reads: () => reads };
}

test('a device callback invalidates upcoming code before interpreter fetch', { timeout: 120000 }, async t => {
  // MOV AL, [0x2000]; INC AX. The callback replaces INC with DEC.
  const { runtime, cpu, reads } = machine(t, [0xa0, 0, 0x20, 0x40]);
  assert.equal((await waitFor(runtime, 0)).kind, 'installed');
  const id = runtime.requestBlock(0x1000, 2);
  assert.equal((await waitFor(runtime, id)).kind, 'installed');
  assert.equal((await runtime.runSlice(2)).exit, 'yielded');
  assert.equal(cpu.getUint32(28, true), 0xffff);
  assert.equal(cpu.getUint32(144, true), 2);
  assert.equal(reads(), 1);
});

test('a RAM alias write invalidates compiled code fetched from ROM', { timeout: 120000 }, async t => {
  // MOV byte [0x3005], 0x48; INC AX.
  const { runtime, cpu, reads } = machine(t, [0xc6, 0x06, 0x05, 0x30, 0x48, 0x40]);
  assert.equal((await waitFor(runtime, 0)).kind, 'installed');
  const id = runtime.requestBlock(0x1000, 2);
  assert.equal((await waitFor(runtime, id)).kind, 'installed');
  assert.equal((await runtime.runSlice(2)).exit, 'yielded');
  assert.equal(cpu.getUint32(28, true), 0xffff);
  assert.equal(cpu.getUint32(144, true), 2);
  assert.equal(reads(), 0);
});
