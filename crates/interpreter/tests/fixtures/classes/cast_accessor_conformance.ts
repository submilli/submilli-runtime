// A structural cast decides "has property `p`" from the data-slot scan. An
// accessor-backed property has no data slot, so it used to read as absent and the
// cast threw — even though reading `.p` through the interface works.
//
// The test accepts the accessor slot's existence without invoking the getter: a
// conformance predicate that ran user code could throw or have side effects.

interface Sized {
  area: number;
}

interface Opt {
  area?: number;
}

interface Outer {
  inner: Sized;
}

class ByAccessor {
  private s: number = 3;
  get area(): number {
    return this.s * this.s;
  }
}

class ByField {
  area: number = 4;
}

class WrongType {
  area: string = "x";
}

class NoArea {
  other: number = 1;
}

class WriteOnly {
  private s: number = 0;
  set area(v: number) {
    this.s = v;
  }
}

class Counting {
  private n: number = 0;
  get area(): number {
    this.n = this.n + 1;
    return 9;
  }
  peek(): number {
    return this.n;
  }
}

class ThrowingGetter {
  get area(): number {
    throw new Error("getter ran");
  }
}

/// Casts and then reads, so a value that conforms but cannot answer the read
/// still counts as failing.
function castThrows(value: unknown): boolean {
  try {
    const s = value as Sized;
    console.log(`${s.area}`);
    return false;
  } catch (e) {
    return true;
  }
}

/// The cast alone — no read, so only the conformance test can throw.
function castOnlyThrows(value: unknown): boolean {
  try {
    const s = value as Sized;
    return false;
  } catch (e) {
    return true;
  }
}

export function main(): string {
  const accessor: unknown = new ByAccessor();
  assert((accessor as Sized).area === 9, "an accessor-backed property conforms");

  const field: unknown = new ByField();
  assert((field as Sized).area === 4, "a data field still conforms");

  const literal: unknown = { area: 7 };
  assert((literal as Sized).area === 7, "an object literal still conforms");

  assert(castThrows(new NoArea()), "an absent property still fails the cast");
  assert(castThrows(new WrongType()), "a wrong-typed data field still fails the cast");
  assert(castThrows(new WriteOnly()), "a setter-only property does not satisfy a readable one");

  const opt = accessor as Opt;
  assert(opt.area === 9, "an optional accessor property conforms and reads");
  const optAbsent = (new NoArea() as unknown) as Opt;
  assert(optAbsent.area === undefined, "an absent optional property still conforms");

  const nested: unknown = { inner: new ByAccessor() };
  assert((nested as Outer).inner.area === 9, "a nested accessor conforms");

  const elems: unknown = [new ByAccessor(), new ByAccessor()];
  assert((elems as Sized[])[1].area === 9, "an array element's accessor conforms");

  // The test accepts the accessor slot's existence; it must not invoke the
  // getter, which can have side effects or throw.
  const counting = new Counting();
  const countingRaw: unknown = counting;
  const counted = countingRaw as Sized;
  assert(counting.peek() === 0, "the cast did not invoke the getter");
  assert(counted.area === 9 && counting.peek() === 1, "reading through the interface invokes it once");
  assert(!castOnlyThrows(new ThrowingGetter()), "a throwing getter does not make the cast throw");

  // `get area` is an ordinary string, so a value can carry a *data* field spelled
  // exactly that. Neither shape is accessor-backed and neither may satisfy `area`.
  const spoofData: unknown = JSON.parse("{\"get area\": 5}");
  assert(castThrows(spoofData), "a data field named `get area` does not satisfy `area`");
  const spoofClosure: unknown = { "get area": (): number => 9 };
  assert(castThrows(spoofClosure), "a closure stored under `get area` does not either");

  return "ok";
}
