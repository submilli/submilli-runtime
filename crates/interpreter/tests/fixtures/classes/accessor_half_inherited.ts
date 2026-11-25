// Redeclaring one half of an accessor replaces the whole prototype descriptor:
// the other half is absent, not inherited from an ancestor. Reading a
// setter-only override through a parent-typed reference gives `undefined`, and
// writing a getter-only override through a parent-typed reference throws a
// `TypeError`.

class GetOnly {
  get g(): string | undefined {
    return "go";
  }
}

class AddsSetter extends GetOnly {
  private w: string = "";
  set g(x: string) {
    this.w = x;
  }
  peek(): string {
    return this.w;
  }
}

class SetOnly {
  private v: string = "s";
  set g(x: string) {
    this.v = x;
  }
  peek(): string {
    return this.v;
  }
}

class AddsGetter extends SetOnly {
  get g(): string {
    return "child-get";
  }
}

class Top {
  get p(): number | undefined {
    return 1;
  }
}
class Middle extends Top {}
class Bottom extends Middle {
  private w: number = 0;
  set p(x: number) {
    this.w = x;
  }
  peek(): number {
    return this.w;
  }
}

class Pair {
  private v: number = 10;
  get n(): number | undefined {
    return this.v;
  }
  set n(x: number) {
    this.v = x;
  }
  peek(): number {
    return this.v;
  }
}
class OverridesGetter extends Pair {
  get n(): number {
    return 99;
  }
}
class OverridesSetter extends Pair {
  private w: number = 0;
  set n(x: number) {
    this.w = x;
  }
  peekW(): number {
    return this.w;
  }
}
class GrandChildKeepsCleared extends OverridesSetter {}
class GrandChildRestoresBoth extends OverridesSetter {
  private g: number = 77;
  get n(): number {
    return this.g;
  }
  set n(x: number) {
    this.g = x;
  }
}

class Holder<T> {
  private v: T;
  constructor(v: T) {
    this.v = v;
  }
  get item(): T | undefined {
    return this.v;
  }
}

class Relay<U> extends Holder<U> {
  private held: U;
  constructor(v: U) {
    super(v);
    this.held = v;
  }
  set item(x: U) {
    this.held = x;
  }
  peek(): U {
    return this.held;
  }
}

function readViaBase(a: GetOnly | null): string | undefined {
  return a?.g;
}

function throwsTypeError(fn: () => void): boolean {
  try {
    fn();
    return false;
  } catch (e) {
    return e instanceof TypeError;
  }
}

export function main(): string {
  const a = new AddsSetter();
  a.g = "hi";
  assert(a.peek() === "hi", "the child's setter ran");
  const baseA: GetOnly = a;
  assert(baseA.g === undefined, "parent-typed read of setter-only override gives undefined");
  assert(readViaBase(a) === undefined, "optional chain through parent also gives undefined");
  assert(readViaBase(null) === undefined, "null still short-circuits");
  assert(JSON.stringify(a) === "{}", "cleared getter is omitted from JSON.stringify");

  const b = new AddsGetter();
  assert(b.g === "child-get", "the child's getter answers");
  const baseB: SetOnly = b;
  assert(
    throwsTypeError(() => {
      baseB.g = "hi";
    }),
    "parent-typed write to getter-only override throws TypeError",
  );
  assert(b.peek() === "s", "base setter did not run");

  const c = new Bottom();
  c.p = 7;
  assert(c.peek() === 7, "the child's setter ran");
  const topC: Top = c;
  assert(topC.p === undefined, "getter two levels up is cleared by setter-only subclass");

  const d = new OverridesGetter();
  assert(d.n === 99, "the overriding getter wins");
  const pairD: Pair = d;
  assert(pairD.n === 99, "parent-typed read dispatches to the overriding getter");
  assert(
    throwsTypeError(() => {
      pairD.n = 5;
    }),
    "parent-typed write throws TypeError when getter-only subclass clears setter",
  );
  assert(d.peek() === 10, "base setter did not run");

  const os = new OverridesSetter();
  os.n = 55;
  assert(os.peekW() === 55, "setter-only override runs its own setter");
  const pairOs: Pair = os;
  assert(pairOs.n === undefined, "parent-typed read of setter-only override yields undefined");
  pairOs.n = 66;
  assert(os.peekW() === 66, "parent-typed write dispatches to setter-only override");

  const gcCleared: Pair = new GrandChildKeepsCleared();
  assert(gcCleared.n === undefined, "grandchild inherits the cleared getter");

  const gcRestored = new GrandChildRestoresBoth();
  const pairGc: Pair = gcRestored;
  assert(pairGc.n === 77, "grandchild declaring both halves restores the getter");
  pairGc.n = 88;
  assert(gcRestored.n === 88, "and its own setter");

  const rn = new Relay<number>(7);
  rn.item = 42;
  assert(rn.peek() === 42, "generic subclass writes through its own setter");
  const hn: Holder<number> = rn;
  assert(hn.item === undefined, "generic setter-only subclass clears inherited getter");

  const rs = new Relay<string>("a");
  rs.item = "b";
  assert(rs.peek() === "b", "second generic instantiation keeps its own setter");
  const hs: Holder<string> = rs;
  assert(hs.item === undefined, "second generic instantiation also clears inherited getter");

  return "ok";
}
