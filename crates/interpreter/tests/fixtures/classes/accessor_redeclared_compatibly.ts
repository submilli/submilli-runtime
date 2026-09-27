// An accessor redeclaring an inherited accessor keeps its kind and fits the
// inherited signature, so it shares the vtable slot and a parent-typed receiver
// dispatches to the child. Narrowing the getter's return and widening the
// setter's parameter are both legal — the same variance a method override gets.

class Animal {
  kind(): string {
    return "animal";
  }
}

class Dog extends Animal {
  kind(): string {
    return "dog";
  }
  fetch(): string {
    return "ball";
  }
}

class Base {
  seen: string = "";
  private pet: Animal | null = null;

  get best(): Animal | null {
    return this.pet;
  }
  set best(a: Animal | null) {
    this.pet = a;
    this.seen = "base";
  }
}

class Sub extends Base {
  private dog: Dog = new Dog();

  // Narrowed return: `Dog` is assignable to `Animal | null`.
  get best(): Dog {
    return this.dog;
  }
  // Widened parameter: still accepts everything the inherited setter did.
  set best(a: Animal | null) {
    this.seen = "sub";
  }
}

// Same type on both sides is the ordinary case.
class Plain {
  get n(): number {
    return 1;
  }
}

class PlainSub extends Plain {
  get n(): number {
    return 2;
  }
}

// A class may redeclare one half of the pair and inherit the other.
class HalfBase {
  private v: string = "half-base";
  get h(): string {
    return this.v;
  }
  set h(x: string) {
    this.v = x;
  }
}

class HalfSub extends HalfBase {
  get h(): string {
    return "half-sub";
  }
}

class GenericBase<T> {
  private t: T;
  constructor(t: T) {
    this.t = t;
  }
  get g(): T {
    return this.t;
  }
}

// Compared at the same opaque parameter on both sides: a passthrough is legal.
class GenericSub<T> extends GenericBase<T> {
  get g(): T {
    return this.own;
  }
  private own: T;
  constructor(t: T) {
    super(t);
    this.own = t;
  }
}

// Three levels, all accessors, narrowed once at the top of the chain.
class L1 {
  get lx(): string | null {
    return "l1";
  }
}

class L2 extends L1 {
  get lx(): string {
    return "l2";
  }
}

class L3 extends L2 {
  get lx(): string {
    return "l3";
  }
}

// A getter narrowed across the boxing boundary: the inherited slot is a boxed
// `number | null`, the override's own type an unboxed `number`.
class NumBase {
  get nx(): number | null {
    return 1;
  }
}

class NumSub extends NumBase {
  get nx(): number {
    return 2;
  }
}

class UnkBase {
  get ux(): unknown {
    return 1;
  }
}

class UnkSub extends UnkBase {
  get ux(): number {
    return 2;
  }
}

// An interface a class *implements* is not an ancestor, so backing one of its
// members with an accessor is not a redeclaration of anything.
interface IfaceOnly {
  readonly ia: string;
  ib: string;
}

class IfaceP {
  other: string = "p";
}

class IfaceC extends IfaceP implements IfaceOnly {
  get ia(): string {
    return "c";
  }
  ib: string = "b";
}

interface HasA {
  readonly a: string;
}

class ABase implements HasA {
  get a(): string {
    return "base";
  }
}

class ASub extends ABase {
  get a(): string {
    return "sub";
  }
}

class APlain implements HasA {
  readonly a: string = "plain";
}

function readA(h: HasA): string {
  return h.a;
}

function readMixed(x: HasA | ASub): string {
  return x.a;
}

// Serialization reads public getters and omits private backing fields.
class SerAcc {
  private sn: number = 4;
  get area(): number {
    return this.sn;
  }
  set area(v: number) {
    this.sn = v;
  }
}

function main(): void {
  const s = new Sub();
  const asBase: Base = s;
  assert(s.best.fetch() === "ball", "the child's narrowed getter reads as `Dog`");
  assert(asBase.best !== null, "a parent-typed read dispatches to the child's getter");

  asBase.best = new Animal();
  assert(s.seen === "sub", "a parent-typed write dispatches to the child's setter");

  assert(new PlainSub().n === 2, "same-typed accessor override");
  const p: Plain = new PlainSub();
  assert(p.n === 2, "parent-typed read of a same-typed override");

  const h = new HalfSub();
  assert(h.h === "half-sub", "the redeclared getter wins");
  const asHalfBase: HalfBase = h;
  asHalfBase.h = "written";
  assert(asHalfBase.h === "half-sub", "the redeclared getter answers a parent-typed read too");

  const l3 = new L3();
  const asL2: L2 = l3;
  const asL1: L1 = l3;
  assert(l3.lx === "l3", "a three-level accessor chain reads the most-derived getter");
  assert(asL2.lx === "l3", "and through the intermediate type");
  assert(asL1.lx === "l3", "and through the root type, whose declaration was nullable");

  const asNumBase: NumBase = new NumSub();
  assert(asNumBase.nx === 2, "a getter narrowed from boxed `number | null` to unboxed `number`");
  assert(new UnkSub().ux === 2, "a getter narrowed from `unknown`");

  const viaIface: IfaceOnly = new IfaceC();
  assert(viaIface.ia === "c", "an accessor backing an interface member no ancestor declared");
  assert(readA(new ASub()) === "sub", "an interface-typed receiver dispatches to the override");
  assert(readMixed(new ASub()) === "sub", "a union receiver, class arm");
  assert(readMixed(new APlain()) === "plain", "a union receiver, interface arm");

  assert(
    JSON.stringify(new SerAcc()) === '{"area":4}',
    "an accessor-backed class serializes its public property",
  );
  assert(JSON.stringify(new ASub()) === '{"a":"sub"}', "an inherited getter uses the override");

  const g = new GenericSub<string>("gen");
  const asGenericBase: GenericBase<string> = g;
  assert(g.g === "gen", "generic passthrough accessor");
  assert(asGenericBase.g === "gen", "parent-typed read of a generic passthrough accessor");
}
