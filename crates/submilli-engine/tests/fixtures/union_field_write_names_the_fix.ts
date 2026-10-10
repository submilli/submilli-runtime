// The write side of a union receiver owes the same narrow-first help the read
// side gives. This pins the message table: `=`, `+=` and `++` all reach one
// reporter, and a name that is a method, a name a member lacks, and a built-in
// property each say which of them it is.
//
// The three operators share a reporter but not a rewrite: a named fix has to be
// the edit the reader makes, and `x.f = …` is a different operation from `x.f++`
// — with the elided right-hand side hiding that the increment was dropped.
// expect-error: cannot assign to `f` through `A | B`: a union receiver has no single field layout to write
// expect-error: narrow to one member first — `if (x instanceof A) { x.f = … }`
// expect-error: narrow to one member first — `if (x instanceof A) { x.f += … }`
// expect-error: narrow to one member first — `if (x instanceof A) { x.f++ }`
// expect-error: narrow to one member first — `if (x instanceof A) { x.f-- }`
// expect-error: field `only` does not exist on all members of `A | B` (missing on `B`)
// expect-error: `m` is a method on `A`; methods are fixed at their declaration
// expect-error: `size` on `Map<string, number>` is a built-in property, which a union receiver cannot assign through
// expect-error: `ro` is readonly on every member; no narrowing makes it writable

class A {
    f: number = 1;
    only: number = 3;
    readonly ro: number = 4;
    size: number = 5;
    m(): number { return 0; }
}
class B {
    f: number = 2;
    readonly ro: number = 6;
    m(): number { return 1; }
}

function pick(flag: boolean): A | B { return flag ? new A() : new B(); }
function orMap(flag: boolean): Map<string, number> | A {
    return flag ? new Map<string, number>() : new A();
}

export function main(): string {
    const x = pick(true);
    x.f = 5;
    x.f += 1;
    x.f++;
    x.f--;
    x.only = 7;
    x.m = 9;
    x.ro = 9;

    const hosted = orMap(true);
    hosted.size = 2;

    // The read on the same receiver compiles, which is the asymmetry this covers.
    return x.f.toString();
}
