import assert from 'node:assert/strict';

export function cpuMemory(profile = 'flat32') {
  const memory = new WebAssembly.Memory({ initial: 1 });
  const cpu = new DataView(memory.buffer);
  cpu.setUint32(60, 0x1000, true);
  for (let segment = 0; segment < 6; segment++) {
    cpu.setUint32(64 + segment * 12 + 4, profile === 'real16' ? 0xffff : 0xffffffff, true);
    cpu.setUint16(64 + segment * 12 + 10, (segment === 1 ? 7 : 5) | (profile === 'real16' ? 0 : 16), true);
  }
  // Default x87 state, matching the public backing ABI.
  new Uint8Array(memory.buffer).fill(1, 152, 158);
  cpu.setUint8(158, 3);
  cpu.setUint16(164, 0xffff, true);
  return memory;
}

export async function waitFor(runtime, id, work = 0) {
  const deadline = Date.now() + 60000;
  while (Date.now() < deadline) {
    const result = await runtime.runSlice(work);
    const event = result.compilations.find(event => event.id === id);
    if (event) return event;
    assert.notEqual(result.exit, 'unavailable');
  }
  assert.fail(`compilation ${id} did not finish`);
}
