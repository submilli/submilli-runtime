// The write-side union diagnostics name a fix; this runs each named fix verbatim,
// so a suggestion that doesn't compile fails here rather than reaching a reader.
// `instanceof` is the class branch (tested on the erased class, never at an
// instantiation), the checked cast is the branch for members with no runtime
// witness.

class A<T> { f: T; constructor(v: T) { this.f = v; } }
class B<T> { f: T; constructor(v: T) { this.f = v; } }
class RO { readonly f: number = 1; }
class RW { f: number = 2; }
interface I1 { f: number; }
interface I2 { f: number; }

function generics(flag: boolean): A<string> | B<string> {
    return flag ? new A<string>("a") : new B<string>("b");
}
function mixedReadonly(flag: boolean): RO | RW { return flag ? new RO() : new RW(); }

interface Shape { size: number; area(): number; }
interface Big { size: number; area(): number; extra: string; }
class S1 implements Shape { size: number = 1; area(): number { return this.size; } }
function upcastable(flag: boolean): Shape | Big {
    return flag ? new S1() : { size: 2, area: (): number => 2, extra: "e" };
}

interface Circle { kind: "circle"; shared: number; }
interface Square { kind: "square"; shared: number; }
function shape(flag: boolean): Circle | Square {
    return flag ? { kind: "circle", shared: 1 } : { kind: "square", shared: 2 };
}

class Priv {
    private hidden: number = 1;
    set(v: number): void { this.hidden = v; }
    get(): number { return this.hidden; }
}
class Priv2 { private hidden: number = 2; }
function priv(flag: boolean): Priv | Priv2 { return flag ? new Priv() : new Priv2(); }
function shapes(flag: boolean): I1 | I2 { return flag ? { f: 1 } : { f: 2 }; }

export function main(): string {
    // `if (x instanceof A) { x.f = … }` — the bare class, not `A<string>`.
    const g = generics(true);
    if (g instanceof A) {
        g.f = "z";
        assert(g.f === "z", "instanceof narrowing admits the write");
    }

    // The named member must be the writable one, not the `readonly` sibling.
    const m = mixedReadonly(false);
    if (m instanceof RW) {
        m.f = 5;
        assert(m.f === 5, "narrowing names a member the write succeeds on");
    }

    // The bind-first fix, for an element — not a place a guard can narrow.
    const arr: (A<string> | B<string>)[] = [new A<string>("a"), new B<string>("b")];
    const bound = arr[0];
    if (bound instanceof A) {
        bound.f = "y";
        assert(bound.f === "y", "binding an element first admits the write");
    }

    // The discriminant fix, named when the union carries one. The literal named
    // is one whose member is writable, so following it verbatim compiles.
    const d = shape(true);
    if (d.kind === "circle") {
        d.shared = 9;
        assert(d.shared === 9, "discriminant narrowing admits the write");
    }

    // A closure-reassigned local reads as a place but the narrowing engine
    // refuses a view for it; binding first is what escapes that.
    let mutated: A<string> | B<string> = new A<string>("m");
    const reset = (): void => { mutated = new B<string>("n"); };
    const snapshot = mutated;
    if (snapshot instanceof A) {
        snapshot.f = "q";
        assert(snapshot.f === "q", "binding escapes a captured mutator");
    }
    reset();

    // `private` is not `readonly`: narrowing admits the write.
    const p = priv(true);
    if (p instanceof Priv) {
        p.set(4);
        assert(p.get() === 4, "a private field is writable once narrowed");
    }

    // The proven-upcast cast: `Shape` is an illegal runtime-check target, and
    // legal here because the source is already assignable to it.
    const up = upcastable(true);
    (up as Shape).size = 5;
    assert((up as Shape).size === 5, "a proven upcast admits the write");

    // `(x as I1).f = …` — the cast branch, for members with no runtime witness.
    const s = shapes(true);
    const narrowed = s as I1;
    narrowed.f = 7;
    assert(narrowed.f === 7, "checked cast admits the write");

    return "ok";
}
