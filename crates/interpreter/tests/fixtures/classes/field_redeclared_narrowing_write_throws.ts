// A narrowing redeclaration shares one storage slot with the declaration it
// narrows, so every write that goes through the parent's declaration can leave
// the slot holding a value the subclass's declaration does not admit. The
// language accepts the narrowing (spec.md §Classes, matching TypeScript); the
// read is what catches it, and it throws a `TypeError` naming the field and both
// declarations rather than raising a bare `cast failure` trap.

class Animal {
  speak(): string {
    return "generic";
  }
}
class Dog extends Animal {
  fetch(): string {
    return "ball";
  }
}

class PB {
  v: Animal | null = null;
  reset(): void {
    this.v = new Animal();
  }
}
class PC extends PB {
  v: Dog | null = new Dog();
}
// A subclass that does not redeclare still reads at the narrowed type, so it
// inherits the guard...
class PD extends PC {}
// ...and so does one that redeclares at the *same* type, which narrows nothing
// of its own and must not drop what the ancestor installed.
class PE extends PC {
  v: Dog | null = new Dog();
}

class StrBase {
  v: string | null = "base";
  clear(): void {
    this.v = null;
  }
}
class StrNarrow extends StrBase {
  v: string = "narrow";
}

type Shape = { area: number };
type Circle = { area: number; r: number };
class ShapeBase {
  v: Shape | null = null;
  widen(): void {
    this.v = { area: 1 };
  }
}
class ShapeNarrow extends ShapeBase {
  v: Circle | null = { area: 1, r: 2 };
}

// Shape-backed interface values are structurally verifiable. Non-shape carriers
// have no member payload to inspect, but every non-null interface declaration
// still rejects the `null` this ancestor can write.
interface Plain {
  n: number;
}
interface Extended {
  n: number;
  m: number;
}
class IfaceBase {
  v: Plain | null = null;
  clear(): void {
    this.v = null;
  }
}
class IfaceNarrow extends IfaceBase {
  v: Extended = { n: 1, m: 2 };
}

// A narrowing to a named object alias records that alias as its shape, and is
// guarded like any inline one — the recorded shape reaches neither the module's
// shape table nor any expression, so its field names have to be registered for
// the field scan on its own.
type SoleShape = { n: number; soleMember: number };
class SoleBase {
  v: { n: number } | number = 0;
  widen(): void {
    this.v = 7;
  }
}
class SoleNarrow extends SoleBase {
  v: SoleShape = { n: 1, soleMember: 2 };
}

type UnknownLeaf = { n: number; u: unknown; m: number };
class UnknownLeafBase {
  v: { n: number } | number = 0;
  widen(): void {
    this.v = 3;
  }
}
class UnknownLeafNarrow extends UnknownLeafBase {
  v: UnknownLeaf = { n: 1, u: "x", m: 2 };
}

class Root {
  v: string | number | null;
  constructor() {
    this.v = null;
  }
  set(x: string | number | null): void {
    this.v = x;
  }
}
class Mid extends Root {
  v: string | null;
  constructor() {
    super();
    this.v = null;
  }
}
class Leaf extends Mid {
  v: string;
  constructor() {
    super();
    this.v = "a";
  }
}

class GBox<T> {
  v: T | null;
  constructor(v: T | null) {
    this.v = v;
  }
}
class GSub<T> extends GBox<T> {
  v: T;
  constructor(v: T) {
    super(v);
    this.v = v;
  }
}

interface WithMethod {
  n: number;
  go(): number;
}
class WithMethodImpl {
  n: number = 1;
  go(): number {
    return 2;
  }
}
class DeepNullOnlyBase {
  v: WithMethod | number | null = null;
  clear(): void {
    this.v = null;
  }
}
class DeepNullOnlyMid extends DeepNullOnlyBase {
  v: WithMethod | null = null;
}
class DeepNullOnlyLeaf extends DeepNullOnlyMid {
  v: WithMethod = new WithMethodImpl();
}

class NullOnlyBase {
  v: WithMethod | null = null;
  clear(): void {
    this.v = null;
  }
}
class NullOnlyNarrow extends NullOnlyBase {
  v: WithMethod = new WithMethodImpl();
}

// `readonly` on the inherited declaration closes every write route: it is
// writable only in its declaring class's constructor, and the subclass's
// initializer runs after that, so the subclass's value wins.
class RoBase {
  readonly v: Animal | null = null;
  constructor() {
    this.v = new Animal();
  }
}
class RoNarrow extends RoBase {
  readonly v: Dog | null = new Dog();
}

function threw(run: () => void): boolean {
  try {
    run();
    return false;
  } catch (e) {
    return e instanceof TypeError;
  }
}

function main(): void {
  // The undisturbed narrowing reads fine — the guard is not a tax on the
  // ordinary case.
  const ok = new PC();
  const okv = ok.v;
  assert(okv !== null && okv.fetch() === "ball", "a narrowed field reads its own value");

  const c = new PC();
  c.reset();
  assert(
    threw(() => {
      const got = c.v;
      console.log(got === null ? "null" : "dog");
    }),
    "an inherited method's write is caught at the narrowed read",
  );

  // The same slot through a parent-typed reference.
  const c2 = new PC();
  const asParent: PB = c2;
  asParent.v = new Animal();
  assert(
    threw(() => {
      const got = c2.v;
      console.log(got === null ? "null" : "dog");
    }),
    "a parent-typed write is caught at the narrowed read",
  );

  // ...and a parent-typed read is unaffected: `Animal | null` admits it.
  const back: PB = c2;
  const seen = back.v;
  assert(seen !== null && seen.speak() === "generic", "the parent-typed read still works");

  const d = new PD();
  d.reset();
  assert(
    threw(() => {
      const got = d.v;
      console.log(got === null ? "null" : "dog");
    }),
    "a subclass of the narrowing class inherits the guard",
  );

  const e = new PE();
  e.reset();
  assert(
    threw(() => {
      const got = e.v;
      console.log(got === null ? "null" : "dog");
    }),
    "redeclaring at the same type keeps the inherited guard",
  );

  const iface = new IfaceNarrow();
  iface.clear();
  assert(
    threw(() => {
      console.log(iface.v.m);
    }),
    "an interface narrowing catches the `null` an ancestor writes",
  );

  // A read at a type *narrower* than the declaration — an optional chain reading
  // through a live field-path narrowing, which drops the `| null` — is guarded
  // against its own type, not just the declaration's. Testing only the
  // declaration would let the `null` this write leaves pass and then trap in the
  // cast, which is the uncatchable failure the guard exists to replace.
  // Calls preserve field guards; a mutation through a call can therefore make
  // the live chain read violate the narrowed type. Direct loop writes instead
  // invalidate the narrowing before typing the next iteration.
  const narrowed = new PC();
  let caughtNarrowed = false;
  if (narrowed.v !== null) {
    clearNarrowed(narrowed);
    try {
      const got = narrowed?.v;
      console.log(got === null ? "null" : "dog");
    } catch (e) {
      caughtNarrowed = e instanceof TypeError;
    }
  }
  assert(caughtNarrowed, "a chain reading at the narrowed type is guarded too");

  // An `unknown` leaf is testable — the test admits every value for it — so the
  // shape around it keeps its guard and the other members still constrain.
  const unk = new UnknownLeafNarrow();
  unk.widen();
  assert(
    threw(() => {
      const got = unk.v;
      console.log(got === null ? "null" : "leaf");
    }),
    "a shape with an `unknown` leaf is still guarded",
  );

  // The guard defends against the *widest* ancestor declaration, not the nearest:
  // the slot is shared with every declaration above, so an intermediate class
  // narrowing it first must not shrink what the leaf's guard covers.
  const leaf = new Leaf();
  leaf.set(42);
  assert(
    threw(() => {
      console.log(leaf.v);
    }),
    "the guard covers what the widest ancestor can write",
  );

  // An erased type parameter may itself admit `null` at some instantiation, so a
  // presence check would reject a value the declaration allows.
  const nullable: GSub<string | null> = new GSub<string | null>(null);
  assert(nullable.v === null, "a `T | null` instantiation reads its legal null");

  // With an intermediate declaration in between, the leaf's type is a null-strip
  // of its *parent* but not of the widest ancestor, so presence is no longer the
  // complete check — and there is no shape to fall back on. A presence check
  // still catches the `null` the ancestor can write, which is what it is for.
  const deep = new DeepNullOnlyLeaf();
  deep.clear();
  assert(
    threw(() => {
      console.log(deep.v.go());
    }),
    "a partial presence check still guards an untestable type",
  );

  // A narrowing that only removes `null` establishes presence and nothing else,
  // so it needs no structural walk — and needs the type to be testable even less.
  // It is the one route by which a method-bearing interface gets a guard.
  const nullOnly = new NullOnlyNarrow();
  nullOnly.clear();
  assert(
    threw(() => {
      console.log(nullOnly.v.go());
    }),
    "a narrowing that only removes `null` is guarded whatever the type",
  );

  const sole = new SoleNarrow();
  assert(sole.v.soleMember === 2, "a narrowing to a named object alias reads its own value");
  sole.widen();
  assert(
    threw(() => {
      console.log(sole.v.soleMember);
    }),
    "a narrowing to a named object alias is guarded like any other",
  );

  const s = new StrNarrow();
  s.clear();
  assert(
    threw(() => {
      console.log(s.v);
    }),
    "a primitive narrowing is guarded too",
  );

  const sh = new ShapeNarrow();
  sh.widen();
  assert(
    threw(() => {
      const got = sh.v;
      console.log(got === null ? "null" : "circle");
    }),
    "a structural narrowing checks the missing field",
  );

  const ro = new RoNarrow();
  const rov = ro.v;
  assert(
    rov !== null && rov.fetch() === "ball",
    "a `readonly` inherited declaration leaves no post-construction write",
  );
}

function clearNarrowed(value: PC): void { value.v = null; }
