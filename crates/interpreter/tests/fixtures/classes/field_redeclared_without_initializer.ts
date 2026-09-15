// A redeclared field shares the parent's payload slot, so an optional
// redeclaration with no initializer of its own would otherwise read back
// whatever the *parent's* initializer left there — a value the redeclaration's
// (possibly narrower) type need not admit. Construction resets the slot, which
// is both what an unshared slot would have held and what ES2022 does: a declared
// field with no initializer defines the property as `undefined`.

class Animal {
  speak(): string {
    return "generic";
  }
}

class Dog extends Animal {
  speak(): string {
    return "woof";
  }
  fetch(): string {
    return "ball";
  }
}

// The narrowing case. `Dog | null` is assignable to `Animal | null`, so the
// redeclaration is legal — but the parent's initializer stores an `Animal`.
class NarrowBase {
  v: Animal | null = new Animal();
}

class Narrowed extends NarrowBase {
  v?: Dog;
}

// Same type on both sides: no unsoundness, but the child still starts empty.
class SameBase {
  p?: string = "parent-init";
}

class SameChild extends SameBase {
  p?: string;
}

// A child that supplies its own initializer keeps it — the reset is only for
// the no-initializer case, and initializers run parent-first.
class WithInit extends NarrowBase {
  v: Dog | null = new Dog();
}

// A constructor assignment runs after field setup, so it wins over the reset.
class CtorAssigned extends NarrowBase {
  v?: Dog;
  constructor() {
    super();
    this.v = new Dog();
  }
}

// A parameter property is auto-assigned, so it is never reset.
class ParamProp extends NarrowBase {
  constructor(public v: Dog | null) {
    super();
  }
}

// The redeclaration may be several levels below the declaring class.
class Middle extends NarrowBase {
  extra: string = "mid";
}

class Leaf extends Middle {
  v?: Dog;
}

// A field the child does *not* redeclare keeps the parent's value.
class Untouched extends NarrowBase {
  other: string = "own";
}

// Every primitive slot type resets, and the parent keeps its own initializers.
class PrimBase {
  pn?: number = 7;
  ps?: string = "p";
  pb?: boolean = true;
}

class PrimChild extends PrimBase {
  pn?: number;
  ps?: string;
  pb?: boolean;
}

class ROBase {
  readonly r?: string = "parent";
}

class ROChild extends ROBase {
  readonly r?: string;
}

// A redeclaration at every level of a three-level chain.
class T1 {
  tv?: string = "t1";
}

class T2 extends T1 {
  tv?: string;
}

class T3 extends T2 {
  tv?: string;
}

// The inherited declaration is a parameter property.
class PPBase {
  constructor(public w: string | null) {}
}

class PPChild extends PPBase {
  w?: string;
  constructor() {
    super("from-parent");
  }
}

class EqB {
  ev?: string = "e";
}

class EqC extends EqB {
  ev?: string;
}

class GB<T> {
  gv?: T;
  constructor(v: T) {
    this.gv = v;
  }
}

class GConcrete extends GB<string> {
  gv?: string;
  constructor() {
    super("p");
  }
}

class GPass<T> extends GB<T> {
  gv?: T;
  constructor(v: T) {
    super(v);
  }
}

// The parent writes the slot from its *constructor body*, not an initializer.
class CtorAssignBase {
  cv: string | null;
  ctag: string;
  constructor() {
    this.cv = "from-ctor";
    this.ctag = "base";
  }
}

class CtorAssignChild extends CtorAssignBase {
  cv?: string;
}

// The reset lands with the child's field setup, which runs after `super()`
// returns — so a virtual call from the parent's constructor still sees the
// parent's value. That ordering is ES2022's and must stay.
class WinBase {
  wv?: string = "parent-init";
  seen: string = "";
  constructor() {
    this.seen = this.probe();
  }
  probe(): string {
    return "base:" + (this.wv === null ? "null" : this.wv);
  }
}

class WinChild extends WinBase {
  wv?: string;
  probe(): string {
    return "child:" + (this.wv === null ? "null" : this.wv);
  }
}

function main(): void {
  const n = new Narrowed();
  assert(n.v === null, "a narrowing redeclaration starts empty, not with the parent's value");
  const asBase: NarrowBase = n;
  assert(asBase.v === null, "the parent-typed read sees the same reset slot");

  const s = new SameChild();
  assert(s.p === null, "a same-typed redeclaration also starts empty");
  assert(new SameBase().p === "parent-init", "the parent itself keeps its initializer");

  const w = new WithInit().v;
  assert(w !== null, "an initializer of the child's own is not reset away");
  if (w !== null) {
    assert(w.fetch() === "ball", "and it holds the child's value");
  }

  const c = new CtorAssigned().v;
  assert(c !== null, "a constructor assignment runs after the reset");
  if (c !== null) {
    assert(c.fetch() === "ball", "and stores the child's value");
  }

  const p = new ParamProp(new Dog()).v;
  assert(p !== null, "a parameter property is auto-assigned, never reset");
  if (p !== null) {
    assert(p.fetch() === "ball", "and holds what the caller passed");
  }

  const l = new Leaf();
  assert(l.v === null, "a redeclaration two levels down resets the same slot");
  assert(l.extra === "mid", "the intermediate class's own field is untouched");

  const u = new Untouched();
  const uv = u.v;
  assert(uv !== null, "a field the child does not redeclare keeps the parent's value");
  if (uv !== null) {
    assert(uv.speak() === "generic", "and the parent's initializer ran");
  }
  assert(u.other === "own", "the child's own new field is initialized");

  assert(JSON.stringify(n) === '{"v":null}', "the reset slot serializes as null");
  assert(JSON.stringify(s) === '{"p":null}', "and so does the same-typed one");

  const pc = new PrimChild();
  assert(pc.pn === null, "an optional number slot resets");
  assert(pc.ps === null, "an optional string slot resets");
  assert(pc.pb === null, "an optional boolean slot resets");
  const pb = new PrimBase();
  assert(pb.pn === 7 && pb.ps === "p" && pb.pb === true, "the parent keeps all three initializers");
  assert(
    JSON.stringify(pc) === '{"pb":null,"pn":null,"ps":null}',
    "reset primitives serialize as null",
  );

  assert(new ROChild().r === null, "a `readonly` optional redeclaration resets");
  assert(new ROBase().r === "parent", "the `readonly` parent keeps its initializer");

  assert(new T3().tv === null, "the deepest level of a chain resets");
  assert(new T2().tv === null, "and so does the middle one");
  assert(new T1().tv === "t1", "the root keeps its initializer");

  assert(
    new PPChild().w === null,
    "an optional redeclaration of an inherited parameter property resets",
  );

  assert(new EqC() === new EqC(), "two reset instances are structurally equal");
  const asEqB: EqB = new EqC();
  assert(!(asEqB === new EqB()), "a reset child is not equal to its parent");

  assert(new GConcrete().gv === null, "a concrete child of a generic parent resets");
  assert(new GPass<string>("x").gv === null, "a generic passthrough resets");

  assert(new CtorAssignBase().cv === "from-ctor", "the parent's constructor assignment stands");
  const cac = new CtorAssignChild();
  assert(cac.cv === null, "the child's redeclaration resets it after `super()` returns");
  assert(cac.ctag === "base", "a field the child does not redeclare keeps the parent's ctor value");

  const win = new WinChild();
  assert(
    win.seen === "child:parent-init",
    "a virtual call from the parent's constructor runs before the child's reset, as ES2022 does",
  );
  assert(win.wv === null, "and the reset lands once the child's field setup runs");
}
