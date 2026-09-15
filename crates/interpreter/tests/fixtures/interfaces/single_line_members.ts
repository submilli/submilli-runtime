// `;` and `,` are interchangeable interface member separators and the last one is
// optional — the same rule inline type literals already followed.
interface Point { x: number; y: number }
interface Named { name: string, tag: string }
interface Greeter { greet(): string }
interface Mixed { id: number, label(): string; }
interface Trailing { a: number, }
interface CallSig { (): number, tag: string }
interface Modifiers { readonly a: number, b?: string; c(): boolean }
interface Gen { m<T>(x: T): T, tag: string }
// The multi-line form, where ASI supplies the separators, is unaffected.
interface Wrapped {
  x: number
  y: number
  m(): number
}

class Hello implements Greeter {
  greet(): string {
    return "hi";
  }
}

class Speaker implements Mixed {
  id: number = 3;
  label(): string {
    return "spoken";
  }
}

function main(): void {
  const p: Point = { x: 1, y: 2 };
  assert(p.x + p.y === 3, "`;` separators, no trailing one");

  const n: Named = { name: "a", tag: "b" };
  assert(n.name + n.tag === "ab", "`,` separators");

  const g: Greeter = new Hello();
  assert(g.greet() === "hi", "method member with no trailing `;`");

  const m: Mixed = new Speaker();
  assert(m.id === 3, "mixed separators, field");
  assert(m.label() === "spoken", "mixed separators, method");

  const t: Trailing = { a: 9 };
  assert(t.a === 9, "trailing separator before `}`");

  const w: Wrapped = new WrappedImpl();
  assert(w.x + w.y + w.m() === 6, "newline-separated members still parse");

  const mo: Modifiers = new ModifiersImpl();
  assert(mo.a === 1 && mo.c(), "readonly / optional / method members on one line");

}

// No instance can be built — v1 has no method-level generics on classes — so this
// signature resolving at all is what proves `Gen`'s member list parsed: an interface
// whose body was rejected would surface here as `unknown type `Gen``.
function tagOf(g: Gen): string {
  return g.tag;
}

class WrappedImpl implements Wrapped {
  x: number = 1;
  y: number = 2;
  m(): number {
    return 3;
  }
}

class ModifiersImpl implements Modifiers {
  readonly a: number = 1;
  b?: string;
  c(): boolean {
    return true;
  }
}

