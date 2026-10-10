// A union reached through an imported *value* arrives without its member types
// imported, so naming one in an `as` or `instanceof` would produce `unknown
// type` — and for the class receiver, on top of the very error the help exists
// to resolve. Neither help may name a type this module cannot spell.
//
// The check covers the whole rendering, not just the head: this module imports
// `Box` but not `Alpha`, so `Box<Alpha>` and `{ f: number; item: Alpha }` are
// unusable here even though `Box` itself resolves.
// expect-error: narrow `s` to one member first
// expect-error: narrow `c` to one member first
// expect-error: narrow `b` to one member first
// expect-error: narrow `t` to one member first
//
// A discriminant is the exception: the test compares a literal, which needs no
// type name, so it is offered here even though no member type is spellable.
// expect-error: `if (d.kind === "a") { d.f = … }`

import { makeShape, makeClassy, makeBoxed, makeStructural, makeDiscriminated, Box } from "./widget";

export function write(): void {
    const s = makeShape();
    s.f = 5;
    const c = makeClassy();
    c.f = 6;
    const b = makeBoxed();
    b.f = 7;
    const t = makeStructural();
    t.f = 8;
    const d = makeDiscriminated();
    d.f = 9;
}
