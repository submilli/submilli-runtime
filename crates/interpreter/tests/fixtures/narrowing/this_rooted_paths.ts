// `this.field` narrows like any other reference path. Before `BindingId::This`
// existed, no `this`-rooted path was ever derived, so every guard on a nullable
// field was a no-op and the read below it failed.

interface Inner {
  s: string | null;
  n: number;
}

class Base {
  inner: Inner | null = null;
}

class Box extends Base {
  label: string | null = null;
  private secret: string | null = null;
  next: Box | null = null;

  setSecret(v: string): void {
    this.secret = v;
  }

  conjunction(): string {
    if (this.inner !== null && this.inner.s !== null) {
      return this.inner.s;
    }
    return "none";
  }

  earlyReturn(): number {
    if (this.inner === null) {
      return -1;
    }
    return this.inner.n;
  }

  ternary(): string {
    return this.label !== null ? this.label.toUpperCase() : "none";
  }

  privateField(): string {
    if (this.secret === null) {
      return "none";
    }
    return this.secret.toUpperCase();
  }

  // Two hops: the region for `this.next.inner` nests inside the one for
  // `this.next`.
  nested(): number {
    if (this.next !== null && this.next.inner !== null) {
      return this.next.inner.n;
    }
    return -1;
  }

  loop(): string {
    let out = "";
    while (this.label !== null) {
      out = out + this.label;
      this.label = null;
    }
    return out;
  }

  // A write to the narrowed path drops the narrowing; re-guarding restores it.
  reguardAfterWrite(): string {
    if (this.label !== null) {
      this.label = null;
    }
    if (this.label !== null) {
      return this.label;
    }
    return "cleared";
  }

  // The closure boundary resets `this`-rooted narrowing (the field is heap
  // state a write can falsify before the body runs), so the body re-guards.
  closureReguards(): string {
    if (this.label !== null) {
      const read = (): string => (this.label === null ? "gone" : this.label);
      return read();
    }
    return "none";
  }
}

// Every predicate form, rooted at `this`.
class Predicates {
  s: string | null = null;
  u: unknown = 1;
  n: number | null = null;
  pet: Animal | null = null;
  r: Ok | Err = { kind: "ok", value: 1 };

  constructor(s: string | null) {
    this.s = s;
    // A guard inside the constructor narrows the same way a method's does.
    if (this.s !== null) {
      this.n = this.s.length;
    }
  }

  truthiness(): string {
    if (this.s) {
      return this.s.toUpperCase();
    }
    return "falsy";
  }

  typeofTag(): string {
    if (typeof this.u === "string") {
      return this.u.toUpperCase();
    }
    return "not-a-string";
  }

  instanceOf(): string {
    if (this.pet instanceof Dog) {
      return this.pet.bark();
    }
    return "not-a-dog";
  }

  discriminant(): string {
    if (this.r.kind === "ok") {
      return `${this.r.value}`;
    }
    return this.r.message;
  }

  bySwitch(): string {
    switch (this.r.kind) {
      case "ok":
        return `ok:${this.r.value}`;
      default:
        return `err:${this.r.message}`;
    }
  }

  chain(): string {
    return this.s?.toUpperCase() ?? "none";
  }
}

interface Animal {
  legs: number;
}

class Dog implements Animal {
  legs: number = 4;
  bark(): string {
    return "woof";
  }
}

interface Ok {
  kind: "ok";
  value: number;
}

interface Err {
  kind: "err";
  message: string;
}

class Generic<T> {
  item: T | null = null;
  or(fallback: T): T {
    if (this.item !== null) {
      return this.item;
    }
    return fallback;
  }
}

function main(): void {
  const b = new Box();
  b.inner = { s: "hi", n: 3 };
  b.label = "L";
  b.setSecret("shh");

  assert(b.conjunction() === "hi", "guard on this.inner then this.inner.s");
  assert(b.earlyReturn() === 3, "inherited field narrowed through this");
  assert(b.ternary() === "L", "ternary guard on this.label");
  assert(b.privateField() === "SHH", "private field narrowed in its own class");
  assert(b.closureReguards() === "L", "closure re-guards the reset path");

  const outer = new Box();
  outer.next = b;
  assert(outer.nested() === 3, "two-hop this path");
  assert(b.nested() === -1, "two-hop guard fails on a null hop");

  assert(b.reguardAfterWrite() === "cleared", "write drops, re-guard restores");
  assert(b.loop() === "", "label already cleared by the write above");

  const fresh = new Box();
  fresh.label = "abc";
  assert(fresh.loop() === "abc", "while-condition guard on this.label");
  assert(fresh.conjunction() === "none", "null field takes the fallback");
  assert(fresh.earlyReturn() === -1, "null field returns early");

  const g = new Generic<string>();
  assert(g.or("f") === "f", "generic field, null");
  g.item = "z";
  assert(g.or("f") === "z", "generic field, narrowed through this");

  const p = new Predicates("hi");
  assert(p.n === 2, "guard inside the constructor");
  assert(p.truthiness() === "HI", "truthiness guard on this.s");
  assert(new Predicates(null).truthiness() === "falsy", "falsy side");
  assert(p.typeofTag() === "not-a-string", "typeof guard on this.u");
  p.u = "u";
  assert(p.typeofTag() === "U", "typeof guard, string side");
  assert(p.instanceOf() === "not-a-dog", "instanceof guard, null side");
  p.pet = new Dog();
  assert(p.instanceOf() === "woof", "instanceof guard on this.pet");
  assert(p.discriminant() === "1", "discriminant on this.r");
  assert(p.bySwitch() === "ok:1", "switch on this.r.kind");
  p.r = { kind: "err", message: "bad" };
  assert(p.discriminant() === "bad", "discriminant, other variant");
  assert(p.bySwitch() === "err:bad", "switch, other variant");
  assert(p.chain() === "HI", "optional chain plus `??` on this.s");
  assert(new Predicates(null).chain() === "none", "optional chain, null side");
}
