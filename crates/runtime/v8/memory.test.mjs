import assert from 'node:assert/strict';
import test from 'node:test';
import { HostMemory } from './memory.mjs';
import { cpuMemory } from './test-support.mjs';

function memory(profile = 'flat32') {
  const cpuState = cpuMemory(profile);
  const guest = new WebAssembly.Memory({ initial: 1 });
  const table = new WebAssembly.Memory({ initial: profile === 'real16' ? 1 : 64 });
  const view = new DataView(table.buffer);
  const cpu = new DataView(cpuState.buffer);
  if (profile === 'real16') {
    view.setUint32(8, 2, true); // ROM code
    view.setUint32(12, 0x1000, true);
    view.setUint32(16, 3, true); // MMIO: capture must stop here
  } else {
    view.setUint32(4, 0x1003, true);
    view.setUint32(8, 0x2003, true);
  }
  new Uint8Array(guest.buffer).fill(0x40, 0x1000, 0x2000);
  const imports = { cpuState, guest, machine: table, physicalMap: table };
  const owner = new HostMemory(imports, profile);
  const watched = page => (new DataView(table.buffer).getUint32(page * (profile === 'real16' ? 8 : 4), true) & 0x10) !== 0;
  return { owner, imports, cpu, watched };
}

test('capture protects pending bytes and every alias before writes', () => {
  const { owner, watched } = memory();
  owner.remap(3, { kind: 'ram', backing: 0x1000, writable: true });
  const job = owner.capture(0x1000, 1);
  assert.equal(job.request.code.length, 15);
  assert(watched(1)); assert(watched(3));
  owner.writeBacking(0x1000, [0x48]);
  assert.equal(job.request.code[0], 0x40, 'worker snapshot is owned');
  assert(!owner.install(job.id));
  assert(!watched(1)); assert(!watched(3));
});

test('watch references survive cancellation and protect newly mapped aliases', () => {
  const { owner, watched } = memory();
  const first = owner.capture(0x1000, 1);
  const second = owner.capture(0x1001, 1);
  owner.cancel(first.id);
  assert(watched(1));
  owner.remap(3, { kind: 'ram', backing: 0x1000, writable: true });
  assert(watched(3));
  assert(owner.install(second.id));
  owner.invalidateWrite(0x3000, 1);
  assert(!owner.contains(second.id));
  assert(!watched(1)); assert(!watched(3));
});

test('pending replacements preserve installed entries and reject remap ABA', () => {
  const { owner, watched } = memory();
  const first = owner.capture(0x1000, 1);
  assert(owner.install(first.id));
  const replacement = owner.capture(0x1000, 2);
  assert.equal(owner.lookup(), first.id);
  owner.cancel(replacement.id);
  assert.equal(owner.lookup(), first.id);
  assert(watched(1));
  const superseded = owner.capture(0x1000, 2);
  const latest = owner.capture(0x1000, 2);
  assert(!owner.install(superseded.id));
  assert(owner.install(latest.id));
  assert(!owner.contains(first.id));
  owner.remap(1, { kind: 'ram', backing: 0x2000, writable: true });
  owner.remap(1, { kind: 'ram', backing: 0x1000, writable: true });
  assert(!owner.contains(latest.id));
  assert(!watched(1));
});

test('capture honors CS limits and rejects changed or non-code contexts', () => {
  const { owner, cpu } = memory('segmented32');
  cpu.setUint32(80, 0x1010, true);
  const job = owner.capture(0x100f, 2);
  assert.equal(job.request.code.length, 2);
  cpu.setUint16(84, 8, true);
  assert(owner.enter());
  assert(!owner.install(job.id));
  assert.equal(owner.capture(0x1011, 1), null);
  cpu.setUint16(86, 31, true); // Invalid kind, with the same default operand size.
  assert.equal(owner.capture(0x1000, 1), null);
});

test('physical snapshots stop before devices and watch writable aliases of ROM', () => {
  const { owner, watched } = memory('real16');
  owner.remap(3, { kind: 'ram', backing: 0x1000, writable: true });
  const job = owner.capture(0x1ffc, 2);
  assert.equal(job.request.code.length, 4);
  assert(watched(1)); assert(watched(3));
  assert.equal(owner.capture(0x2000, 1), null);
  owner.invalidateWrite(0x3000, 1);
  assert(!owner.install(job.id));
});

test('memory growth refreshes views and callback writes cannot add watches', () => {
  const { owner, imports, watched } = memory();
  for (const memory of [imports.cpuState, imports.guest, imports.machine]) memory.grow(1);
  const job = owner.capture(0x1000, 1);
  assert(watched(1));
  owner.invoke(() => {
    assert.throws(() => owner.capture(0x1000, 1), /execution boundary/);
    owner.writeBacking(0x1000, [0x48]);
  });
  assert(!owner.install(job.id));
  assert(!watched(1));
});

test('memories have one owner and closing releases watches for adoption', () => {
  const { owner, imports, watched } = memory();
  assert.throws(() => new HostMemory(imports, 'flat32'), /already have an execution owner/);
  owner.capture(0x1000, 1);
  owner.close();
  assert(!watched(1));
  const next = new HostMemory(imports, 'flat32');
  assert.throws(() => owner.writeBacking(0x1000, [0x48]), /closed/);
  assert.throws(() => owner.remap(1, { kind: 'unmapped' }), /closed/);
  assert(next.capture(0x1000, 1));
  next.close();
});

test('explicit code ranges register complete dependencies without capture', () => {
  const { owner, watched } = memory();
  owner.remap(3, { kind: 'ram', backing: 0x1000, writable: true });
  const ticket = owner.register(0x1000, [{ offset: 0x1000, bytes: 1 }, { offset: 0x2000, bytes: 2 }]);
  assert(owner.isPending(ticket));
  assert(watched(1) && watched(2) && watched(3));
  assert(owner.install(ticket));
  assert(!owner.isPending(ticket));
  assert(!owner.install(ticket));
  assert.equal(owner.lookup(), ticket);
  owner.invalidateWrite(0x2001, 1);
  assert(!owner.contains(ticket));
  assert(!watched(1) && !watched(2) && !watched(3));
});

test('incomplete registrations leave existing tickets and watches unchanged', () => {
  const { owner, watched } = memory();
  const pending = owner.capture(0x1000, 1);
  const incomplete = [{ offset: 0x1000, bytes: 1 }, { offset: 0x2fff, bytes: 2 }];
  assert.equal(owner.register(0x1000, incomplete), null);
  assert(owner.isPending(pending.id));
  assert(!watched(2));
  assert(owner.install(pending.id));
  assert.equal(owner.register(0x1000, incomplete), null);
  assert.equal(owner.register(0x1000, []), null);
  assert.equal(owner.register(0x1000, [{ offset: 0x1000, bytes: 0 }]), null);
  assert.equal(owner.register(0x1000, [{ offset: 0x2000, bytes: 1 }]), null);
  assert.equal(owner.lookup(), pending.id);
});

test('registration requires full CS coverage while capture accepts a prefix', () => {
  const { owner, cpu, watched } = memory('segmented32');
  cpu.setUint32(80, 0x1000, true);
  assert.equal(owner.register(0x1000, [{ offset: 0x1000, bytes: 2 }]), null);
  assert(!watched(1));
  assert.equal(owner.capture(0x1000, 1).request.code.length, 1);
});

test('wrapping registrations protect both sides and reject remapping', () => {
  const { owner, watched } = memory();
  for (const page of [0, 0xfffff]) owner.remap(page, { kind: 'ram', backing: 0x2000, writable: true });
  const ticket = owner.register(0xfffffff8, [{ offset: 0xfffffff8, bytes: 16 }]);
  assert(watched(0) && watched(0xfffff) && watched(2));
  owner.remap(0, { kind: 'unmapped' });
  assert(!owner.install(ticket));
  assert(!watched(0xfffff));
});
