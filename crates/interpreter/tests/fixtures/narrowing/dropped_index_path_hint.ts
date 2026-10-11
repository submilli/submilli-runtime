// expect-error: cannot preserve this guard
// The guard's key is narrowed, but the narrowed source is rebuilt from the
// key's declared type, which can't index, so the guard is dropped with a hint.
class Leaf { z: number | null = 3; }
const key: string | null = "k";
function main(): number {
  const leaves: Record<string, Leaf> = { k: new Leaf() };
  if (key !== null && leaves[key] !== undefined && leaves[key].z !== null) {
    const n: number = leaves[key].z;
    return n;
  }
  return 0;
}
