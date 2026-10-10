// `is Uint8Array` and `as Uint8Array` pin the structural `ref.test` that
// `instanceof Uint8Array` now shares (SUB-663). Before this fixture the
// `Type::Uint8Array` arm in `codegen/cast_check.rs` had no end-to-end coverage,
// so a change to one route could have moved the other silently.

function isBytes(x: unknown): x is Uint8Array {
  return x instanceof Uint8Array;
}

function main(): void {
  const bytes: Uint8Array = new Uint8Array([4, 5, 6]);
  const opaque: unknown = bytes;

  assert(isBytes(opaque), "user guard accepts bytes");
  assert(!isBytes("bytes"), "user guard rejects a string");
  assert(!isBytes(11), "user guard rejects a number");
  assert(!isBytes([4, 5, 6]), "user guard rejects a number[]");

  if (isBytes(opaque)) {
    assert(opaque[1] === 5, "guard narrows enough to index");
  } else {
    assert(false, "guard should have narrowed — never reached");
  }

  const back: Uint8Array = opaque as Uint8Array;
  assert(back.length === 3, "as round-trips through unknown");
  assert(back[2] === 6, "round-tripped bytes are still indexable");

  const notBytes: unknown = "bytes";
  let caught: string = "<none>";
  try {
    const bad: Uint8Array = notBytes as Uint8Array;
    assert(false, "cast should have thrown — never reached");
  } catch (e: Error) {
    caught = e.message;
  }
  assert(
    caught === "type mismatch: expected Uint8Array, got string",
    "failed Uint8Array cast throws a catchable TypeError"
  );
}
