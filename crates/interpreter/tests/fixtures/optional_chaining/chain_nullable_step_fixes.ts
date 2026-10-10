// The other half of `expect_error_chain_nullable_step.ts`: both fixes that
// diagnostic offers have to compile and run, for every step kind — and the
// plain steps that are *not* rejected have to keep working, including the
// whole-chain short-circuit that makes them safe.
type MaybeInner = Inner | null;
type Produce = () => number;

class Inner {
  y: number = 2;
}

class Mid {
  i: Inner = new Inner();
  arr: Inner[] = [new Inner()];
  fn(): Inner {
    return this.i;
  }
}

class Outer {
  b: Inner | null = new Inner();
  arr: MaybeInner[] = [new Inner()];
  produce: Produce | null = (): number => 5;
  m(): Inner | null {
    return this.b;
  }
  mid: Mid = new Mid();
}

function main(): void {
  const o: Outer | null = new Outer();

  // fix 1: continue the chain with `?.`
  assert(o?.b?.y === 2, "?. after a nullable field");
  assert(o?.arr?.[0]?.y === 2, "?. after a nullable element");
  assert(o?.produce?.() === 5, "?. on a nullable callee");
  assert(o?.m()?.y === 2, "?. after a nullable method result");

  // fix 2: assert non-null with `!`
  assert(o?.b!.y === 2, "! after a nullable field");
  assert(o?.arr[0]!.y === 2, "! after a nullable element");
  assert(o?.produce!() === 5, "! on a nullable callee");
  assert(o?.m()!.y === 2, "! after a nullable method result");

  // Plain steps on a *non-nullable* receiver are not rejected — the chain's
  // short-circuit still covers them, because it skips the whole chain.
  assert(o?.mid.i.y === 2, "plain field steps after ?.");
  assert(o?.mid.arr[0].y === 2, "plain index step after ?.");
  assert(o?.mid.fn().y === 2, "plain call step after ?.");

  const none: Outer | null = null as Outer | null;
  assert(none?.mid.i.y === undefined, "the whole chain short-circuits");
  assert(none?.mid.arr[0].y === undefined, "including through an index step");
  assert(none?.mid.fn().y === undefined, "and through a call step");
}
