// `instanceof Uint8Array` narrowing. The true branch reaches byte reads and
// `.length` with no further cast; negation leaves the false branch un-narrowed
// while keeping the true branch usable.

function firstByte(x: unknown): number {
  if (x instanceof Uint8Array) {
    return x[0];
  }
  return -1;
}

function byteCount(x: unknown): number {
  if (!(x instanceof Uint8Array)) {
    return -1;
  }
  return x.length;
}

function main(): void {
  const bytes: Uint8Array = new Uint8Array([7, 8, 9]);

  assert(firstByte(bytes) === 7, "narrowed index read in the true branch");
  assert(firstByte("nope") === -1, "false branch falls through");

  assert(byteCount(bytes) === 3, "negated guard narrows after the early return");
  assert(byteCount(42) === -1, "negated guard takes the early return");

  // A statically-nullable operand is the one shape whose Wasm value type is a
  // nullable concrete ref rather than `$Object`, so `ref.test` sees the null
  // directly instead of through a boxed `unknown`.
  const some: Uint8Array | null = new Uint8Array([1, 2]);
  const none: Uint8Array | null = null;
  assert(some instanceof Uint8Array, "non-null member of a nullable tests true");
  assert(!(none instanceof Uint8Array), "null member of a nullable tests false");
  if (some instanceof Uint8Array) {
    assert(some[1] === 2, "the guard narrows the null away");
  }
}
