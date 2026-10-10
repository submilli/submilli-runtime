// Writing a property through an interface-typed receiver, across every way the
// property can be backed at runtime: a data slot, a `set` accessor, a `get` with
// no `set` (the target exists but is read-only, so the write throws rather than
// vanishing), and absent entirely (an optional member the value never
// materialised, where the write creates a data slot).
//
// The value expression must be evaluated exactly once whichever branch runs —
// including the inserting one, since the source wrote a call.
interface Bag {
  tag: string;
  note?: string;
}

class Stored implements Bag {
  tag: string = "stored";
  note?: string;
}

class Accessor implements Bag {
  tag: string = "accessor";
  private held: string = "init";
  get note(): string {
    return this.held;
  }
  set note(v: string) {
    this.held = v;
  }
}

// Deliberately no `implements Bag`: a get-only accessor is rejected against a
// writable interface member, so structural assignment below is the only way to
// reach the read-only write branch at all.
class ReadOnly {
  tag: string = "readonly";
  get note(): string {
    return "fixed";
  }
}

let calls: number = 0;

function nextValue(): string {
  calls = calls + 1;
  return `v${calls}`;
}

function put(b: Bag): void {
  b.note = nextValue();
}

function main(): void {
  const stored: Bag = new Stored();
  put(stored);
  assert(stored.note === "v1", "data slot takes the write");

  const accessor: Bag = new Accessor();
  assert(accessor.note === "init", "accessor read before the write");
  put(accessor);
  assert(accessor.note === "v2", "setter slot takes the write");

  const readOnly: Bag = new ReadOnly();
  let threw = false;
  let message = "";
  try {
    put(readOnly);
  } catch (e) {
    threw = e instanceof TypeError;
    message = e.message;
  }
  assert(threw, "writing a getter-backed property with no setter throws");
  assert(
    message === "cannot assign to a property backed by a getter with no setter",
    `the error says which property write failed and why, got: ${message}`,
  );
  assert(readOnly.note === "fixed", "and leaves the property alone");
  assert(calls === 3, "the throwing branch still evaluated its value once");

  // Reaches `Bag` by width subtyping, so no `note` slot exists on the value.
  const raw = { tag: "raw" };
  const absent: Bag = raw;
  put(absent);
  assert(absent.note === "v4", "absent optional gets a new data slot");
  assert(calls === 4, "the inserting branch evaluated its value exactly once");
}
