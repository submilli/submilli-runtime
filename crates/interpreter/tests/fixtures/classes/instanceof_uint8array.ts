// `instanceof Uint8Array` (SUB-663). Both construction forms reach the same
// runtime type; every non-byte-array value reached through `unknown` falls out
// false through the structural `ref.test`, including `null` and arrays.

class Thing {
  tag: string;
  constructor() {
    this.tag = "thing";
  }
}

function check(x: unknown): boolean {
  return x instanceof Uint8Array;
}

function main(): void {
  assert(new Uint8Array([1, 2, 3]) instanceof Uint8Array, "new constructs a Uint8Array");
  assert(Uint8Array.alloc(8) instanceof Uint8Array, "alloc constructs a Uint8Array");

  assert(check(new Uint8Array([1, 2, 3])), "bytes through unknown");
  assert(check(Uint8Array.alloc(0)), "empty bytes still test true");

  // Derived views are a third construction origin: whatever struct `subarray`
  // and `slice` hand back has to canonicalize to the same `$Uint8Array` the
  // structural test targets, or a view reads as "not bytes".
  const parent: Uint8Array = new Uint8Array([1, 2, 3, 4]);
  assert(check(parent.subarray(1, 3)), "subarray view is still bytes");
  assert(check(parent.slice(1, 3)), "slice copy is still bytes");

  assert(!check("bytes"), "string is not bytes");
  assert(!check(42), "number is not bytes");
  assert(!check(true), "boolean is not bytes");
  assert(!check({ length: 3 }), "object literal is not bytes");
  assert(!check([1, 2, 3]), "number[] is not bytes");
  assert(!check(new Map<string, number>()), "Map is not bytes");
  assert(!check(new Thing()), "class instance is not bytes");
  assert(!check(null), "null is not bytes");
}
