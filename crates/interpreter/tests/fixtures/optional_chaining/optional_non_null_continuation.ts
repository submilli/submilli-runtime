// `!` continues an optional chain (`a?.b!.y`). Inside a chain it means what it
// means outside one: a runtime-checked narrowing that throws `TypeError` on
// null, not TypeScript's unchecked assertion.
//
// Position decides what it asserts. Mid-chain it asserts one step, and the chain
// keeps its own `| null`, so a base that short-circuits skips the assertion
// entirely. At the tail there is no later step to feed, and the only null left
// to remove is the one the short-circuit adds — so a trailing `!` asserts the
// whole chain, exactly as TypeScript's `(a?.b)!` does, and a short-circuit
// throws rather than yielding null.
type MaybeInner = Inner | null;
type Produce = () => number;

class Inner {
  y: number = 2;
  self(): Inner {
    return this;
  }
}

class Outer {
  b: Inner | null = new Inner();
  arr: MaybeInner[] = [new Inner()];
  fn: Produce | null = (): number => 5;
  m(): Inner | null {
    return this.b;
  }
}

class Falsy {
  zero: number = 0;
  empty: string = "";
  no: boolean = false;
  zeroBig: bigint = 0n;
}

function f2(): Outer | null {
  return new Outer();
}

function main(): void {
  const present: Outer | null = new Outer();
  assert(present?.b!.y === 2, "asserted step reads through");
  assert(present?.arr[0]!.y === 2, "after an index step");
  assert(present?.m()!.y === 2, "after a method call");
  assert(present?.b!.self().y === 2, "method call after the assertion");
  assert(present?.fn!() === 5, "assertion on the callee of a chain call");
  assert(present?.b!!.y === 2, "repeated assertion");
  assert(present?.b!?.y === 2, "optional step after an assertion");

  const short: Outer | null = null;
  assert(short?.b!.y === null, "base short-circuits before the assertion");

  // A trailing `!` removes the chain's own `| null`, so this is `Inner`, not
  // `Inner | null` — no narrowing needed to read `.y`.
  const tail: Inner = present?.b!;
  assert(tail.y === 2, "trailing assertion narrows the whole chain");

  // Repeated trailing `!` — the lift pops every trailing assertion, not just
  // the last one, and asserts the chain once.
  const twice: Inner = present?.b!!;
  assert(twice.y === 2, "repeated trailing assertion");

  let shortThrew = false;
  try {
    const t: Inner = short?.b!;
    console.log("unreachable", t.y);
  } catch (e) {
    shortThrew = e instanceof TypeError;
  }
  assert(shortThrew, "a trailing assertion throws on the short-circuit");

  // The lift boxes the value to `ref.is_null`-test it, so a falsy primitive
  // tail is where a bad box/unbox would surface as a spurious `TypeError`.
  const falsy = new Falsy();
  const f: Falsy | null = falsy;
  assert(f?.zero! === 0, "trailing assertion on 0");
  assert(f?.empty! === "", "trailing assertion on the empty string");
  assert(f?.no! === false, "trailing assertion on false");
  assert(f?.zeroBig! === 0n, "trailing assertion on 0n");

  // The `NonNull` step inside a closure body, which walks a different path
  // through the capture pass than the same chain in a statement.
  const read = (): number | null => f2()?.b!.y;
  assert(read() === 2, "assertion inside a closure");

  const emptied = new Outer();
  emptied.b = null;
  const nullable: Outer | null = emptied;
  let threw = false;
  try {
    const y = nullable?.b!.y;
    console.log("unreachable", y);
  } catch (e) {
    threw = e instanceof TypeError;
  }
  assert(threw, "asserting a null step throws a catchable TypeError");
}
