// The write reporter treats a receiver as a union of shapes only when every
// member bears fields — the same `is_field_bearing` gate the read side applies.
// A member carrying no fields fails for a different reason, so it is never told
// to narrow with `instanceof`, which would not help it.
//
// `null` is the one such member with a fix of its own: removing it leaves a
// receiver that carries fields, so the guard *is* the answer and gets named.
// `string | A` has no such answer — a guard would land on a second rejection —
// so it keeps the bare message.
// expect-error: cannot assign to field `f` of `A | null`: the receiver can be `null`
// expect-error: guard first — `if (n !== null) { n.f = … }` — or assert non-null with `n!.f = …`
// expect-error: cannot assign to field of `string | A`

class A { f: number = 1; }

function orNull(flag: boolean): A | null { return flag ? new A() : null; }
function orString(flag: boolean): A | string { return flag ? new A() : "s"; }

export function main(): string {
    const n = orNull(true);
    n.f = 5;
    const s = orString(true);
    s.f = 5;
    return "x";
}
