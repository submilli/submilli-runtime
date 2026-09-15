// A subclass redeclaring an inherited field shadows it into ONE property
// sharing ONE payload slot, at the parent's index. Two slots would make a
// parent-typed receiver — including `this` inside an inherited method — read an
// index the child's initializer never wrote.

class P {
  v: string = "p";
  q(): string {
    return this.v;
  }
}

class C extends P {
  v: string = "c";
}

class A {
  v: string = "a";
  who(): string {
    return this.v;
  }
}
class B extends A {
  v: string = "b";
}
class D extends B {
  v: string = "d";
}

// Shadowing at a narrower type: the child's value still satisfies the parent's
// declared type at a parent-typed read.
class NarrowBase {
  v: string | null = null;
}
class Narrowed extends NarrowBase {
  v: string = "narrow";
}

// Interleaved own fields: the shadowed slot keeps the parent's index while the
// child's genuinely-new fields append after the whole inherited prefix.
class Wide {
  a: string = "wa";
  v: string = "wv";
}
class Wider extends Wide {
  v: string = "cv";
  z: string = "cz";
}

class Counter {
  n: number = 1;
  bump(): void {
    this.n += 10;
  }
  read(): number {
    return this.n;
  }
}
class SubCounter extends Counter {
  n: number = 100;
}

// A private field shadows an inherited one the same way: the typechecker's
// chain walk stops at the most-derived declaration whatever its visibility, so
// codegen has to agree and share the slot.
class PrivBase {
  private v: string = "pb";
  peek(): string {
    return this.v;
  }
}
class PrivSub extends PrivBase {
  private v: string = "ps";
}

// A parameter property declares a field like any other, so it shadows like one.
class ParamBase {
  v: string = "pb";
  read(): string {
    return this.v;
  }
}
class ParamSub extends ParamBase {
  constructor(public v: string) {
    super();
  }
}

// ...and shadowing a field the *parent* declared as a parameter property.
class ParamDeclBase {
  constructor(public w: string) {}
  readW(): string {
    return this.w;
  }
}
class ParamDeclSub extends ParamDeclBase {
  w: string = "child-w";
}

// Both sides parameter properties: the child's assignment runs after `super`'s.
class BothBase {
  constructor(public x: string) {}
  readX(): string {
    return this.x;
  }
}
class BothSub extends BothBase {
  constructor(public x: string) {
    super("from-parent");
  }
}

// Shadowing at a class type, narrowed: the parent-typed read still dispatches
// on the child's instance.
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
class PetOwner {
  pet: Animal = new Animal();
}
class DogOwner extends PetOwner {
  pet: Dog = new Dog();
}

// A generic parent instantiated at a concrete type, shadowed at that same type.
class GBox<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
  peek(): T {
    return this.v;
  }
}
class StrBox extends GBox<string> {
  v: string = "strbox";
  constructor() {
    super("parent");
  }
}

// A class that shadows some inherited fields, leaves others, and adds new ones.
class Multi {
  a: string = "a";
  b: string = "b";
  c: string = "c";
}
class MultiSub extends Multi {
  a: string = "sa";
  c: string = "sc";
  d: string = "sd";
}

interface HasV {
  v: string;
}
class IfaceShadow extends P implements HasV {
  v: string = "iface";
}

// Genuine generic passthrough: the child's `T` is the parent's `T` once both
// sides stand at the same parameter, so this is a legal shadow and must not be
// rejected by the opaque comparison that catches the incompatible cases.
class PassBase<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
  peek(): T {
    return this.v;
  }
}
class PassSub<T> extends PassBase<T> {
  v: T;
  constructor(v: T) {
    super(v);
    this.v = v;
  }
}

function main(): void {
  const c = new C();
  const b: P = c;
  assert(c.v === "c", "child-typed read sees the child's initializer");
  assert(b.v === "c", "parent-typed read sees the child's value, not slot 0");
  assert(c.q() === "c", "`this` inside an inherited method reads the shared slot");

  const d = new D();
  const asB: B = d;
  const asA: A = d;
  assert(d.v === "d", "three-level chain: most-derived initializer wins");
  assert(asB.v === "d", "middle-typed read of a three-level chain");
  assert(asA.v === "d", "root-typed read of a three-level chain");
  assert(d.who() === "d", "inherited method on a three-level chain");

  const n: NarrowBase = new Narrowed();
  assert(n.v === "narrow", "shadowing at a narrower type reads through the parent");

  const w = new Wider();
  const asWide: Wide = w;
  assert(w.a === "wa", "inherited field the child does not redeclare");
  assert(w.v === "cv", "shadowed field keeps the parent's index");
  assert(w.z === "cz", "the child's own new field appends after the prefix");
  assert(asWide.a === "wa", "parent-typed read of a non-shadowed field");
  assert(asWide.v === "cv", "parent-typed read of the shadowed field");
  assert(
    JSON.stringify(w) === '{"a":"wa","v":"cv","z":"cz"}',
    "the shadowed field serializes once, with the child's value",
  );

  const sc = new SubCounter();
  const asCounter: Counter = sc;
  sc.n = sc.n + 1;
  asCounter.n += 5;
  sc.bump();
  sc.n++;
  assert(sc.n === 117, "writes through both static types land in one slot");
  assert(asCounter.n === 117, "parent-typed read after mixed writes");
  assert(sc.read() === 117, "inherited method reads the same slot it wrote");

  assert(new PrivSub().peek() === "ps", "a private field shadows an inherited one");

  const pp = new ParamSub("param-v");
  const ppAsBase: ParamBase = pp;
  assert(pp.v === "param-v", "a parameter property shadows an inherited field");
  assert(ppAsBase.v === "param-v", "parent-typed read of a parameter-property shadow");
  assert(pp.read() === "param-v", "inherited method reads the parameter property's slot");

  const pd = new ParamDeclSub("ignored");
  const pdAsBase: ParamDeclBase = pd;
  assert(pd.w === "child-w", "a field shadows an inherited parameter property");
  assert(pdAsBase.w === "child-w", "parent-typed read over a shadowed parameter property");
  assert(pd.readW() === "child-w", "the parent's own method reads the child's value");

  const bs = new BothSub("child-x");
  const bsAsBase: BothBase = bs;
  assert(bs.x === "child-x", "parameter property over parameter property");
  assert(bsAsBase.x === "child-x", "the child's assignment lands after super()'s");
  assert(bs.readX() === "child-x", "inherited method over a doubly-declared param property");

  const dogOwner = new DogOwner();
  const asOwner: PetOwner = dogOwner;
  assert(asOwner.pet.speak() === "woof", "a class-typed field shadowed at a subtype");
  assert(dogOwner.pet.fetch() === "ball", "the child's narrower type is visible to it");

  const sb = new StrBox();
  const sbAsBox: GBox<string> = sb;
  assert(sb.v === "strbox", "shadowing a generic parent's field at its instantiated type");
  assert(sbAsBox.v === "strbox", "generic-parent-typed read of the shadowed field");
  assert(sb.peek() === "strbox", "the generic parent's method reads the shared slot");

  const m = new MultiSub();
  assert(
    JSON.stringify(m) === '{"a":"sa","b":"b","c":"sc","d":"sd"}',
    "some fields shadowed, one inherited untouched, one appended",
  );

  const viaIface: HasV = new IfaceShadow();
  assert(viaIface.v === "iface", "an interface-typed receiver resolves the shared slot");

  const pass = new PassSub<string>("pass");
  const passAsBase: PassBase<string> = pass;
  assert(pass.v === "pass", "generic passthrough is a legal shadow");
  assert(passAsBase.v === "pass", "parent-typed read of a passthrough shadow");
  assert(pass.peek() === "pass", "the generic parent's method reads the shared slot");

  assert(new C() === new C(), "structural equality over the deduped payload");
  const plainParent: P = new P();
  const childAsParent: P = new C();
  assert(!(plainParent === childAsParent), "a shadowing child is not equal to its parent");
}
