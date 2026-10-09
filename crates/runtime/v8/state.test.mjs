import assert from 'node:assert/strict';
import test from 'node:test';
import { compatible, eip } from './state.mjs';
import { cpuMemory } from './test-support.mjs';

test('profile admission checks actual segment caches and refreshed memory views', () => {
  const memory = cpuMemory();
  let cpu = new DataView(memory.buffer);
  assert(compatible(memory, 'flat32'));
  assert(compatible(memory, 'segmented32'));
  assert(!compatible(memory, 'segmented16'));
  cpu.setUint32(64, 0x1000, true); // ES base breaks flat assumptions only.
  assert(!compatible(memory, 'flat32'));
  assert(compatible(memory, 'segmented32'));
  cpu.setUint16(86, 7, true);
  assert(compatible(memory, 'segmented16'));
  assert(!compatible(memory, 'segmented32'));
  for (let segment = 0; segment < 6; segment++) {
    cpu.setUint32(64 + segment * 12, 0, true);
    cpu.setUint32(64 + segment * 12 + 4, 0xffff, true);
    cpu.setUint16(64 + segment * 12 + 10, segment === 1 ? 7 : 5, true);
  }
  assert(compatible(memory, 'real16'));
  cpu.setUint16(86, 0x107, true); // Real mode requires canonical reserved bits.
  assert(!compatible(memory, 'real16'));
  cpu.setUint16(86, 7, true);
  memory.grow(1);
  cpu = new DataView(memory.buffer);
  cpu.setUint32(60, 0x87654321, true);
  assert.equal(eip(memory), 0x87654321);
  assert(compatible(memory, 'real16'));
});
