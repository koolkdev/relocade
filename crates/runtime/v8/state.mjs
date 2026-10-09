// CPU backing ABI; memory views are refreshed because hosts may grow memories.
export function eip(memory) { return new DataView(memory.buffer).getUint32(60, true); }

export function compatible(memory, profile) {
  const cpu = new DataView(memory.buffer);
  const attributes = segment => cpu.getUint16(64 + segment * 12 + 10, true);
  const flat = (segment, kind) => {
    const offset = 64 + segment * 12;
    return cpu.getUint32(offset, true) === 0
      && cpu.getUint32(offset + 4, true) === 0xffffffff
      && (attributes(segment) & 15) === kind;
  };
  const real = segment => {
    const offset = 64 + segment * 12;
    return cpu.getUint32(offset, true) === cpu.getUint16(offset + 8, true) * 16
      && cpu.getUint32(offset + 4, true) === 0xffff
      && attributes(segment) === (segment === 1 ? 7 : 5);
  };
  const codeBig = (attributes(1) & 16) !== 0;
  return codeBig === !['segmented16', 'real16'].includes(profile)
    && (profile !== 'real16' || [0, 1, 2, 3, 4, 5].every(real))
    && (profile !== 'flat32' || (flat(1, 7) && [0, 2, 3].every(s => flat(s, 5))
      && (attributes(2) & 16) !== 0));
}
