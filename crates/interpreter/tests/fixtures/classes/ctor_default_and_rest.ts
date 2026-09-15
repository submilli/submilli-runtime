enum Tone {
  Soft = 0,
  Loud = 1,
}

enum Shade {
  Pale = "pale",
  Deep = "deep",
}

interface Named {
  label(): string;
}

class Note implements Named {
  constructor(readonly n: number) {}
  label(): string {
    return `n${this.n}`;
  }
}

class Defaults {
  constructor(
    readonly name: string,
    readonly scale: number = 2,
    readonly tags: string[] = [],
  ) {}
}

// Every `DefaultValue` kind the resolver can produce, in constructor position.
class Kinds {
  constructor(
    readonly flag: boolean = true,
    readonly below: number = -3,
    readonly peer: Note | null = null,
    readonly named: Named | null = null,
    readonly tone: Tone = Tone.Loud,
    readonly shade: Shade = Shade.Deep,
  ) {}
}

// `fixed_count == 0`: the rest tail is drained from index 0.
class Bag {
  count: number;
  constructor(...ns: number[]) {
    this.count = ns.length;
  }
}

// Reference-typed rest elements — a different array element ABI than `number[]`.
class Notes {
  total: number;
  first: string;
  constructor(...notes: Note[]) {
    let t = 0;
    for (const note of notes) {
      t += note.n;
    }
    this.total = t;
    this.first = notes.length === 0 ? "none" : notes[0].label();
  }
}

class Labels {
  joined: string;
  constructor(...named: Named[]) {
    let s = "";
    for (const one of named) {
      s += one.label();
    }
    this.joined = s;
  }
}

// Two defaulted fixed params ahead of the rest: the fill slice is longer than
// one slot and interleaves with the drain.
class Wide {
  a: number;
  b: string;
  tail: number;
  constructor(a: number = 1, b: string = "b", ...ns: Array<number>) {
    let t = 0;
    for (const n of ns) {
      t += n;
    }
    this.a = a;
    this.b = b;
    this.tail = t;
  }
}

class WideChild extends Wide {
  constructor() {
    super(5);
  }
}

class Failure extends Error {
  details: string;
  constructor(message: string = "failed", ...parts: string[]) {
    super(message);
    this.details = parts.join(",");
  }
}

class Base {
  total: number;
  label: string;
  constructor(label: string = "base", ...ns: number[]) {
    let t = 0;
    for (const n of ns) {
      t += n;
    }
    this.total = t;
    this.label = label;
  }
  sum(bonus: number = 10, ...extra: number[]): number {
    let t = this.total + bonus;
    for (const e of extra) {
      t += e;
    }
    return t;
  }
}

class Child extends Base {
  constructor() {
    super();
  }
  viaSuper(): number {
    return super.sum();
  }
  viaSuperRest(): number {
    return super.sum(1, 2, 3);
  }
}

class Forward extends Base {}

// Two levels of implicit forwarding, and `super.method()` resolving to a
// grandparent's defaulted + variadic signature.
class Mid extends Base {}
class Leaf extends Mid {
  viaGrandparent(): number {
    return super.sum();
  }
}

class Box<T> {
  first: T;
  count: number;
  constructor(first: T, ...rest: T[]) {
    this.first = first;
    this.count = rest.length;
  }
}

class Tagged<T> {
  value: T;
  tag: string;
  constructor(value: T, tag: string = "none") {
    this.value = value;
    this.tag = tag;
  }
}

class SubTagged extends Tagged<number> {
  constructor(v: number) {
    super(v);
  }
}

// A generic parent whose defaulted param is `T[]`: `EmptyArray` resolves its
// element type from an erased slot.
class Store<T> {
  items: T[];
  constructor(items: T[] = []) {
    this.items = items;
  }
}

class NumberStore extends Store<number> {
  constructor() {
    super();
  }
}

// `new` outside a plain statement: static and instance field initializers each
// emit through their own path.
class Sites {
  static readonly seed: Bag = new Bag();
  readonly own: Bag = new Bag(1, 2);
  static make(): Wide {
    return new Wide();
  }
}

const topLevel: Defaults = new Defaults("top");

function main(): void {
  const d = new Defaults("a");
  assert(d.scale === 2, "omitted defaulted ctor param");
  assert(d.tags.length === 0, "omitted defaulted array ctor param");
  const d2 = new Defaults("b", 5, ["x"]);
  assert(d2.scale === 5 && d2.tags[0] === "x", "explicit args past the defaults");

  const b = new Base("hi", 1, 2, 3);
  assert(b.total === 6 && b.label === "hi", "rest ctor param collects the tail");
  const b2 = new Base();
  assert(b2.total === 0 && b2.label === "base", "default filled, rest empty");
  const b3 = new Base("only");
  assert(b3.total === 0 && b3.label === "only", "explicit fixed arg, empty rest");

  const c = new Child();
  assert(c.viaSuper() === 10, "`super.method()` fills the omitted default");
  assert(c.viaSuperRest() === 6, "`super.method()` packs the rest tail");

  const f = new Forward("fwd", 4);
  assert(f.total === 4 && f.label === "fwd", "implicit ctor forwards default+rest");
  const f2 = new Forward();
  assert(f2.total === 0 && f2.label === "base", "implicit ctor with no args");

  const g = new Box<number>(1, 2, 3);
  assert(g.first === 1 && g.count === 2, "generic ctor rest tail");
  const g2 = new Box<string>("a");
  assert(g2.count === 0, "generic ctor empty rest tail");
  const t = new Tagged<number>(7);
  assert(t.tag === "none", "generic ctor default");
  const s = new SubTagged(4);
  assert(s.value === 4 && s.tag === "none", "`super()` into a generic parent's default");
  assert(new NumberStore().items.length === 0, "`T[] = []` default through `super()`");

  const k = new Kinds();
  assert(k.flag === true, "boolean default");
  assert(k.below === -3, "negative number default");
  assert(k.peer === null, "`null` default in a class-typed slot");
  assert(k.named === null, "`null` default in an interface-typed slot");
  assert(k.tone === Tone.Loud, "number enum variant default");
  assert(k.shade === Shade.Deep, "string enum variant default");

  assert(new Bag().count === 0, "rest-only ctor with no args");
  assert(new Bag(1, 2, 3).count === 3, "rest-only ctor collects the whole tail");

  const notes = new Notes(new Note(1), new Note(2));
  assert(notes.total === 3 && notes.first === "n1", "class-typed rest elements");
  assert(new Notes().total === 0, "class-typed rest, empty");
  assert(new Labels(new Note(1), new Note(2)).joined === "n1n2", "interface-typed rest elements");

  const w = new Wide();
  assert(w.a === 1 && w.b === "b" && w.tail === 0, "two defaults filled, rest empty");
  const w2 = new Wide(5);
  assert(w2.a === 5 && w2.b === "b" && w2.tail === 0, "second default filled");
  const w3 = new Wide(5, "z");
  assert(w3.a === 5 && w3.b === "z" && w3.tail === 0, "both defaults supplied");
  const w4 = new Wide(5, "z", 7, 8);
  assert(w4.tail === 15, "`Array<number>` rest spelling collects the tail");
  const wc = new WideChild();
  assert(wc.a === 5 && wc.b === "b" && wc.tail === 0, "`super(a)` fills the rest of the tail");

  const leaf = new Leaf("leaf", 2);
  assert(leaf.total === 2, "implicit ctor forwards through two levels");
  assert(leaf.viaGrandparent() === 12, "`super.method()` reaching a grandparent");

  const fail = new Failure();
  assert(fail.message === "failed", "`Error` subclass default forwarded to `super`");
  const fail2 = new Failure("bad", "x", "y");
  assert(fail2.message === "bad" && fail2.details === "x,y", "`Error` subclass rest tail");

  assert(Sites.seed.count === 0, "`new` in a static field initializer");
  assert(new Sites().own.count === 2, "`new` in an instance field initializer");
  assert(Sites.make().a === 1, "`new` in a static method body");
  assert(topLevel.scale === 2, "`new` in a top-level const initializer");
}
