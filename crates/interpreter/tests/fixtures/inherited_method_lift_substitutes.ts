// A lifted method signature is rendered with the bindings member *resolution*
// produced, not a table zipped from the receiver's own generics. An inherited
// method is written in its declaring class's parameter names, so a receiver-side
// table prints those names back at a reader who never wrote them.
// expect-error: StringBox.put(x: string): void
// expect-error: Flip<string, number>.set(k: number, v: string): void
// expect-error: Pair<string, number>.set(k: string, v: number): void
// A partially-applied base, and a two-level chain: neither is a reorder, and both
// need the walk to know what the declaring class's parameters are bound to.
// expect-error: Half<number>.set(k: number, v: string): void
// expect-error: Deep.put(x: string): void
// An override resolves at the subclass, so its own parameter names apply.
// expect-error: Renamed.put(y: string): void
// The universal vtable methods answer with empty bindings and must not be
// disturbed by the substitution path.
// expect-error: StringBox.toString(): string
// The *call* route reaches a different reporting site than a bare reference, and
// has to print the same substituted lift.
// expect-error: method `put` expects 1 argument(s), got 0
// expect-error: method `set` expects 2 argument(s), got 0

class Box<T> {
    private v: T;
    constructor(v: T) { this.v = v; }
    put(x: T): void { this.v = x; }
}

// No generics at all, so a receiver-side table is empty and `Box`'s raw `T`
// survives untouched.
class StringBox extends Box<string> {}

// Two levels between the receiver and the declaration.
class Deep extends StringBox {}

// An override: resolution stops here, so `y` is the name to print.
class Renamed extends Box<string> {
    put(y: string): void {}
}

class Pair<K, V> {
    private k: K;
    private v: V;
    constructor(k: K, v: V) { this.k = k; this.v = v; }
    set(k: K, v: V): void { this.k = k; this.v = v; }
}

// Binds `A`/`B` while the inherited signature spells `K`/`V`, and swaps their
// order through the `extends` clause: a non-empty receiver-side table misses too,
// and the right answer needs the chain walk.
class Flip<A, B> extends Pair<B, A> {}

// Partial application: one parameter rides through, one is fixed at the clause.
class Half<T> extends Pair<T, string> {}

export function main(): string {
    const s = new StringBox("a");
    const bad1 = s.put;
    s.put();
    const bad2 = s.toString;

    const f = new Flip<string, number>(1, "a");
    const bad3 = f.set;
    f.set();

    // The un-inherited case, holding the ordinary path in place beside them.
    const p = new Pair<string, number>("a", 1);
    const bad4 = p.set;

    const h = new Half<number>(1, "a");
    const bad5 = h.set;

    const d = new Deep("a");
    const bad6 = d.put;

    const r = new Renamed("a");
    const bad7 = r.put;

    return "x";
}
