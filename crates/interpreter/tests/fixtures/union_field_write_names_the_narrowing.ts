// Which fix `union_write_fix_help` names, per member shape. Every form here is
// run verbatim in `union_field_write_named_fix_round_trip.ts`.
//
// `instanceof` tests the erased class, so it names the bare `A`, never
// `A<string>`; a `readonly` member is not a miss, so it must not be named as the
// narrowing target; `private` is not `readonly`, so the write succeeds once
// narrowed; a discriminated union names a literal whose own member is writable
// (here `Circle.shared` is readonly, so the guard must name `"square"`),
// since a bare `…` lets a reader pick a branch that then rejects the write.
// expect-error: `if (x instanceof A) { x.f = … }`
// expect-error: `if (p instanceof Priv) { p.hidden = … }`
// expect-error: `if (d.kind === "square") { d.shared = … }`
//
// A mixed class/interface union leads with `instanceof` — it narrows there too,
// and a guard that picks the wrong member fails its test where a cast traps. The
// cast follows as the alternative, named against the interface member because
// `as` rejects a class target.
// expect-error: `if (mixed instanceof Klass) { mixed.shared = … }`, or write through a checked cast — `(mixed as Iface).shared = …`
//
// Members that disagree about the field's type get no cast: it would compile and
// then reject the value, trading this error for a mismatch. And a member `as`
// itself rejects is never named — that legality question is asked of `as`.
// expect-error: narrow `split` to one member first
// expect-error: write through a checked cast — `(withMethod as Plain).f = …`
//
// A member `as` accepts only as a *proven upcast* is still offered: a method-
// bearing interface is an illegal runtime-check target, but casting to it from a
// source already assignable to it emits no check, so it compiles.
// expect-error: write through a checked cast — `(up as Shape).size = …`

class A { f: number = 1; }
class B { f: number = 2; }
function pick(flag: boolean): A | B { return flag ? new A() : new B(); }

class Priv { private hidden: number = 1; }
class Priv2 { private hidden: number = 2; }
function priv(flag: boolean): Priv | Priv2 { return flag ? new Priv() : new Priv2(); }

interface Circle { kind: "circle"; readonly shared: number; }
interface Square { kind: "square"; shared: number; }
function shape(flag: boolean): Circle | Square {
    return flag ? { kind: "circle", shared: 1 } : { kind: "square", shared: 2 };
}

interface Iface { shared: number; }
class Klass { shared: number = 1; }
function mix(flag: boolean): Klass | Iface { return flag ? new Klass() : { shared: 2 }; }

interface NumF { v: number; }
interface StrF { v: string; }
function splitTyped(flag: boolean): NumF | StrF { return flag ? { v: 1 } : { v: "a" }; }

interface Shape { size: number; area(): number; }
interface Big { size: number; area(): number; extra: string; }
class S1 implements Shape { size: number = 1; area(): number { return this.size; } }
function upcastable(flag: boolean): Shape | Big {
    return flag ? new S1() : { size: 2, area: (): number => 2, extra: "e" };
}

interface HasMethod { f: number; m(): number; }
interface Plain { f: number; p: string; }
function methodBearing(flag: boolean): HasMethod | Plain {
    return flag ? { f: 1, m: (): number => 1 } : { f: 2, p: "y" };
}

export function main(): string {
    const x = pick(true);
    x.f = 5;

    const p = priv(true);
    p.hidden = 5;

    const d = shape(true);
    d.shared = 5;

    const mixed = mix(true);
    mixed.shared = 5;

    const split = splitTyped(true);
    split.v = 9;

    const withMethod = methodBearing(true);
    withMethod.f = 5;

    const up = upcastable(true);
    up.size = 5;
    return "x";
}
