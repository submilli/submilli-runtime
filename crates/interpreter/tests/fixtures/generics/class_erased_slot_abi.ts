// A class method's vtable slot lowers class types, type variables, and unions
// containing either to the boxed object slot — independently of whether the
// class's struct type happens to be recorded yet. These shapes exercise every
// slot position that erasure decides: a param, a return, and both through an
// override of a generic parent.
class Inner {
  n: number = 1;
}

class Plain {
  // Union of a class with a primitive: a ref slot that is not a class type.
  pick(x: Inner | string): number {
    return typeof x === "string" ? x.length : x.n;
  }
  // Returning the receiver's own class — the slot erases, the call site casts.
  self(): Plain {
    return this;
  }
  // A prelude class as a parameter, reconstructed before local slot sigs.
  describe(e: Error): string {
    return e.message;
  }
}

class Slot<T> {
  private current: T | null = null;
  put(v: T | null): void {
    this.current = v;
  }
  taken(): T | null {
    return this.current;
  }
}

class NumSlot extends Slot<number> {
  // Concrete annotations over erased parent slots.
  taken(): number | null {
    return super.taken();
  }
}

function main(): void {
  const p = new Plain();
  assert(p.pick("abc") === 3, "class|string union param");
  assert(p.pick(new Inner()) === 1, "class member of the same union");
  assert(p.self().pick("xy") === 2, "self-returning method chains");
  assert(p.describe(new Error("boom")) === "boom", "prelude class param");

  const s = new NumSlot();
  assert(s.taken() === null, "erased nullable return, empty");
  s.put(7);
  const got = s.taken();
  assert(got !== null && got === 7, "primitive through an erased nullable slot");
}
