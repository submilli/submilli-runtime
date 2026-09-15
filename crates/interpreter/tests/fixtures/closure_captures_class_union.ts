// A closure capturing a value whose type is a union of classes. The closure
// env struct is typed before class type indices are recorded, so such a field
// erases to a *nullable* `$Object`, while the body's local resolves to the
// non-null form once the indices exist — the load has to close that gap.
//
// Narrowing makes this reachable without the user writing a capture at all: a
// `const` narrowing seeds a region into every closure lexically inside it, and
// the region's own read of the root is a capture.
class Ok {
  constructor(readonly value: number) {}
}

class Err {
  constructor(readonly message: string) {}
}

function useIt(v: Ok | Err): number {
  return v instanceof Ok ? v.value : -1;
}

function describe(results: Array<Ok | Err>): string {
  const parts: string[] = [];
  for (const r of results) {
    if (r instanceof Ok) {
      // a callback inside an `instanceof`-narrowed region
      parts.push([r.value].map((n: number): string => n.toString()).join(""));
    } else {
      parts.push("E:" + r.message);
    }
  }
  return parts.join(",");
}

function main(): void {
  // explicit capture, no narrowing anywhere
  const x: Ok | Err = new Ok(7);
  const direct = (): number => useIt(x);
  assert(direct() === 7, "explicit capture of a class-union value");

  // a closure inside the region that never mentions the narrowed value
  if (x instanceof Ok) {
    const unrelated = (): number => 1;
    assert(unrelated() === 1, "closure seeded but not referencing the root");
    const reading = (bump: number): number => x.value + bump;
    assert(reading(1) === 8, "closure reading the narrowed root");
  } else {
    assert(false, "x is Ok");
  }

  assert(describe([new Ok(1), new Err("bad"), new Ok(2)]) === "1,E:bad,2");
}
