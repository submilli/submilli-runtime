// `null | T` is the receiver a model most often leaves unguarded, and the generic
// "non-object type" message reads as though the receiver is the wrong *kind* of
// thing rather than as a missing guard — it never puts the word `null` anywhere a
// reader can act on. Both sides of the assignment name the fix now.
//
// Every branch names forms that actually compile, which is a stronger test than
// "the receiver spells null": the fix is claimed only when removing the `null`
// makes *this* access legal. Advice that lands on a second rejection is worse
// than the generic message it replaces, and costs the reader that message's
// type dump as well.
// expect-error: cannot read field `f` on `null | A`: the receiver can be `null`
// expect-error: guard first — `if (x !== null) { … x.f … }` — or read it as `x?.f`, or assert non-null with `x!.f`
// expect-error: cannot assign to field `f` of `null | A`: the receiver can be `null`
// expect-error: guard first — `if (y !== null) { y.f = … }` — or assert non-null with `y!.f = …`
// A nullable *interface* receiver reaches the same pair.
// expect-error: cannot read field `g` on `null | I`: the receiver can be `null`
// A read-modify-write names its own operator: `p!.f = …` is a different edit
// from `p.f++`, and a rewrite that compiles but is not the edit still costs a
// round trip.
// expect-error: guard first — `if (c !== null) { c.f += … }` — or assert non-null with `c!.f += …`
// expect-error: guard first — `if (d !== null) { d.f++ }` — or assert non-null with `d!.f++`
// A method is read through a call, so its rewrites carry it — `x?.m` alone is a
// method reference, which is not a value here. The arity rides along: a method
// that takes arguments gets `(…)`, because `a!.map()` is a fix that does not
// compile and would silently drop the arguments the reader already wrote.
// expect-error: guard first — `if (m !== null) { … m.go() … }` — or read it as `m?.go()`, or assert non-null with `m!.go()`
// expect-error: guard first — `if (arr !== null) { … arr.map(…) … }` — or read it as `arr?.map(…)`, or assert non-null with `arr!.map(…)`
// An array, a string and a `Map` are the commonest nullable receivers in
// generated code, and their members are properties rather than fields.
// expect-error: cannot read field `length` on `string | null`: the receiver can be `null`

// --- shapes where the guard is NOT the fix, and the generic message stands ---

// The non-null half carries no members at all.
// expect-error: cannot read field `f` on non-object type `number | null`
// The field is absent on what remains, so every rewrite lands on a second error.
// expect-error: cannot read field `nothere` on non-object type `null | A`
// A union write has no single field layout to reach, so neither a guard nor `!`
// makes it legal — unlike the union *read* just below it, which both fix.
// expect-error: cannot assign to field of `null | A | B`
// One member of the surviving union cannot back the field, and a union read
// rejects wholesale.
// expect-error: cannot read field `f` on non-object type `null | I | A`
// A `readonly` field and a getter with no setter are both readable and neither
// is assignable, so the write side asks the *write* authority, not the read one.
// expect-error: cannot assign to field of `null | RO`
// expect-error: cannot assign to field of `null | Getter`

// --- a guard needs a place to live on ---

// expect-error: an element is not a place a guard can narrow — bind `elems[0]` to a `const` first and guard that, or assert non-null with `elems[0]!.f = …`
// expect-error: bind `(maybeA(true))` to a `const` first and guard that, or read it as `(maybeA(true))?.f`, or assert non-null with `(maybeA(true))!.f`

class A { f: number = 1; }
class B { f: number = 2; }
interface I { g: number; }
class M { go(): number { return 1; } }
class RO { readonly r: number = 1; }
class Getter {
    private n: number = 0;
    get v(): number { return this.n; }
}
// An accessor pair may take wider than it returns, so the hint a rejected write
// passes its value has to be the *setter's* type — checking `null` against the
// getter's `number` would add an error the program has not earned.
class Widening {
    private n: number = 0;
    get w(): number { return this.n; }
    set w(s: number | null) { this.n = s === null ? 0 : s; }
}

function maybeA(flag: boolean): A | null { return flag ? new A() : null; }
function maybeI(flag: boolean): I | null { return flag ? { g: 1 } : null; }
function maybeN(flag: boolean): number | null { return flag ? 1 : null; }
function maybeM(flag: boolean): M | null { return flag ? new M() : null; }
function maybeAB(flag: boolean): A | B | null { return flag ? new A() : null; }
function maybeIA(flag: boolean): I | A | null { return flag ? new A() : null; }
function maybeRO(flag: boolean): RO | null { return flag ? new RO() : null; }
function maybeGetter(flag: boolean): Getter | null { return flag ? new Getter() : null; }
function maybeWidening(flag: boolean): Widening | null { return flag ? new Widening() : null; }
function maybeArr(flag: boolean): number[] | null { return flag ? [1] : null; }
function maybeStr(flag: boolean): string | null { return flag ? "s" : null; }

export function main(): string {
    const x = maybeA(true);
    const read = x.f;

    const y = maybeA(true);
    y.f = 5;

    const i = maybeI(true);
    const readI = i.g;

    const m = maybeM(true);
    const readM = m.go();

    const arr = maybeArr(true);
    const readArr = arr.map((e: number): number => e + 1);

    const str = maybeStr(true);
    const readStr = str.length;

    const ro = maybeRO(true);
    ro.r = 2;

    const g = maybeGetter(true);
    g.v = 2;

    // One error, not two: the write is rejected, and `null` is a legal value for
    // the setter, so the value has nothing to answer for.
    const wide = maybeWidening(true);
    wide.w = null;

    const c = maybeA(true);
    c.f += 1;

    const d = maybeA(true);
    d.f++;

    const n = maybeN(true);
    const readN = n.f;

    const absent = maybeA(true);
    const readAbsent = absent.nothere;

    // The write is refused a fix; the read on the same shape gets one.
    const u = maybeAB(true);
    u.f = 5;

    const mixed = maybeIA(true);
    const readMixed = mixed.f;

    // An element: a guard on `elems[0]` compiles nowhere useful.
    const elems: (A | null)[] = [new A()];
    elems[0].f = 5;

    // A call result: no place at all, so nothing holds a narrowing.
    const readCall = maybeA(true).f;

    return "x";
}
