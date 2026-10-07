// Physical routing and byte-backed MMIO devices for execution tests.
export default function physicalMemory(guest, input, events) {
  const pages = new Map(input.physical_pages.map(([page, backing, writable]) => [page, [writable ? 1 : 2, backing]]));
  const devices = new Map(input.mmio_pages);
  for (const [page] of devices) pages.set(page, [3, 0]);
  const physicalMap = new WebAssembly.Memory({ initial: 1 });
  const table = new DataView(physicalMap.buffer);
  for (const [page, [kind, backing]] of pages) {
    if (page < 0 || page >= 272) throw new Error('physical page exceeds the real-mode table');
    table.setUint32(page * 8, kind, true);
    table.setUint32(page * 8 + 4, backing, true);
  }
  let updates = 0;

  function deviceBacking(address) {
    const page = address >>> 12;
    if (page >= 272 || table.getUint32(page * 8, true) !== 3) {
      throw new Error('callback must stay within MMIO');
    }
    if (!devices.has(page)) throw new Error('missing test device');
    return devices.get(page) + (address & 4095);
  }

  function applyUpdate() {
    const update = input.mmio_updates[updates];
    if (!update) return;
    updates++;
    for (const [memory, patches] of [[guest, update.guest], [physicalMap, update.map]]) {
      for (const [offset, bytes] of patches) new Uint8Array(memory.buffer).set(bytes, offset);
    }
  }

  function checkSpan(address, bytes) {
    if (bytes < 1 || bytes > 8 || address + bytes > 0x110000) {
      throw new Error('invalid MMIO request span');
    }
  }

  return {
    imports: {
      physicalMap,
      readMmio(address, bytes) {
        address >>>= 0;
        checkSpan(address, bytes);
        if (input.observe_mmio) events.push({ kind: 'mmio_read', address, bytes });
        // Poison ignored upper bits to verify partial-transfer masking.
        let value = bytes === 8 ? 0n : -1n << BigInt(bytes * 8);
        for (let offset = 0; offset < bytes; offset++) {
          const byte = new Uint8Array(guest.buffer)[deviceBacking(address + offset)];
          value |= BigInt(byte) << BigInt(offset * 8);
        }
        applyUpdate();
        return BigInt.asIntN(64, value);
      },
      writeMmio(address, bytes, value) {
        address >>>= 0;
        checkSpan(address, bytes);
        if (bytes !== 8 && value >> BigInt(bytes * 8) !== 0n) {
          throw new Error('narrow writes must be zero-extended');
        }
        const contents = Array.from({ length: bytes }, (_, index) => Number((value >> BigInt(index * 8)) & 255n));
        if (input.observe_mmio) events.push({ kind: 'mmio_write', address, value: contents });
        for (let offset = 0; offset < bytes; offset++) {
          new Uint8Array(guest.buffer)[deviceBacking(address + offset)] = contents[offset];
        }
        applyUpdate();
      },
    },
    checkComplete() {
      if (updates !== input.mmio_updates.length) throw new Error('unused MMIO updates');
    },
  };
}
