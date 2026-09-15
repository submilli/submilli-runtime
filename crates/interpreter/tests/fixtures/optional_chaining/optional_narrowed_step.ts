// A chain step resolves its receiver against the narrowing store, not just the
// declared type: a `receiver.field` path a guard has already proven non-null is
// admitted, as the same `o.b.y` outside a chain is. Every step of a chain
// answers this the same way its base does.
//
// Admits, not necessarily reads the same value: outside a chain the read becomes
// the guard's shadow local, while a step re-reads the slot. Every case here keeps
// the narrowing live between the guard and the read, so the two agree.

class Inner {
  y: number = 2;
}
class Mid {
  i: Inner | null = new Inner();
}
class Outer {
  b: Inner | null = new Inner();
  m: Mid | null = new Mid();
}

type OInner = { y: number };
type OOuter = { b: OInner | null };

function mkOuter(): OOuter {
  return { b: { y: 4 } };
}

class Holder {
  elems: (Inner | null)[] = [new Inner()];
  inner: Inner | null = new Inner();
  first(): number | null {
    if (this.inner !== null) {
      return this?.inner.y;
    }
    return -1;
  }
}

class Deep {
  mids: Mid[] = [new Mid()];
}

class Tagged {
  v: number | string = 7;
}

class Animal {}
class Dog extends Animal {
  fetch(): string {
    return "ball";
  }
}
class Kennel {
  pet: Animal = new Dog();
}

function main(): void {
  const o: Outer | null = new Outer();
  if (o !== null && o.b !== null) {
    assert(o.b.y === 2, "the plain read is the control");
    assert(o?.b.y === 2, "a narrowed step is admitted in chain position");
    assert(o?.b!.y === 2, "a `!` after a narrowed step still works");
  }

  const oo: OOuter | null = mkOuter();
  if (oo !== null && oo.b !== null) {
    assert(oo?.b.y === 4, "a narrowed step on an object-shape receiver");
  }

  const deep = new Outer();
  if (deep.m !== null && deep.m.i !== null) {
    assert(deep?.m.i.y === 2, "two narrowed steps in one chain");
  }

  // A write invalidates the narrowing, so the step falls back to the declared
  // type and the chain has to carry its own `?.` again. Rejecting the step
  // without the `?.` reports what killed the narrowing, the way the plain read
  // does — see `expect_error_chain_nullable_step.ts`.
  const w = new Outer();
  if (w.b !== null) {
    w.b = null;
    assert(w?.b?.y === null, "the step reads its declared type after a write");
  }

  // A narrowing whose path *ends* at an index is refused — no shadow local can
  // be synthesized for an element, and the plain `h.elems[0].y` is rejected
  // under the same guard — so the step after one needs its own `?.`.
  const h = new Holder();
  if (h.elems[0] !== null) {
    assert(h?.elems[0]?.y === 2, "a narrowing at an index step is not applied");
  }

  // A path that merely passes *through* an index still narrows at the steps
  // after it, which is what the plain read does.
  const d = new Deep();
  if (d.mids[0].i !== null) {
    assert(d.mids[0].i.y === 2, "the plain read through an index");
    assert(d?.mids[0].i.y === 2, "a narrowed step whose path crosses an index");
  }

  // A `this`-rooted path keys the store like any other root.
  assert(new Holder().first() === 2, "a `this`-rooted narrowed step");

  // Narrowing that is not about null: the step reads at the narrowed type, and
  // codegen casts the slot to it.
  const t = new Tagged();
  if (typeof t.v === "number") {
    const tv: number | null = t?.v;
    assert(tv === 7, "a `typeof`-narrowed step");
  }
  const k = new Kennel();
  if (k.pet instanceof Dog) {
    assert(k?.pet.fetch() === "ball", "an `instanceof`-narrowed step");
  }
}
