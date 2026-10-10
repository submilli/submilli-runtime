// A rejected field write still infers its right-hand side — downstream passes
// need a well-formed node, and the value's *own* errors are a second real
// problem the reader needs in the same pass. What the hint changes is whether
// those errors are accurate: a literal whose inference is hint-driven is
// otherwise inferred at the wrong shape and blames itself for the receiver's
// problem. `[2, "y"]` is a perfectly good `[number, string]`; with no hint it is
// inferred as a homogeneous array and its second element is checked against the
// first's type.
//
// So every rejecting arm passes the field's shape. *Shape*, not permission: a
// `readonly` field still says what a value written to it would have to be, and
// one member being readonly must not cost the others their hint. Withholding it
// trades one beside-the-point error — an accurate mismatch on a write that can
// never be legal — for two false ones.
// expect-error: cannot assign to `pair` through `A | B`: a union receiver has no single field layout to write
// expect-error: expected `string`, got `number`
// expect-error: cannot assign to field `pair` of `A | null`: the receiver can be `null`
// The value's own errors are never dropped — suppressing them would hide a
// second real problem behind the first.
// expect-error: unresolved identifier `notDefinedAnywhere`
// A readonly member does not abort the fold, so the tuple below still types and
// the cast advice — the only form that compiles here — is still named.
// expect-error: write through a checked cast — `(m as MA).pair = …`
// The cast rewrite carries the operator too — a union receiver's help and a
// nullable receiver's help must not disagree about which edit is being named.
// expect-error: write through a checked cast — `(k as MA).count += …`
// expect-error: write through a checked cast — `(j as MA).count++`
// A `string`, an array, a tuple and a `Uint8Array` reach their members through
// the property lookup rather than a field map, and `infer_assign_field` reaches
// them the same way. Adding `| null` must not change what the value is checked
// against, so the shape has to come from that lookup too: answering by variant
// drops it.
// expect-error: cannot assign to readonly property `length` on `string`
// expect-error: cannot assign to field of `string | null`
// expect-error: expected `number`, got `string`

interface A { pair: [number, string]; a: string; }
interface B { pair: [number, string]; b: string; }
// Same field, readonly on one side only.
interface MA { pair: [number, string]; count: number; m: string; }
interface MB { readonly pair: [number, string]; count: number; n: string; }
// Readonly on a nullable receiver: no write is legal, and the shape is still
// what types the value.
class RO { readonly pair: [number, string] = [1, "a"]; }

function ab(flag: boolean): A | B {
    return flag ? { pair: [1, "a"], a: "a" } : { pair: [1, "b"], b: "b" };
}
function maybeA(flag: boolean): A | null {
    return flag ? { pair: [1, "a"], a: "a" } : null;
}
function mixed(flag: boolean): MA | MB {
    return flag ? { pair: [1, "a"], count: 1, m: "m" } : { pair: [1, "b"], count: 2, n: "n" };
}
function maybeRO(flag: boolean): RO | null { return flag ? new RO() : null; }

function plainString(s: string): void { s.length = "x"; }
function nullableString(v: string | null): void {
    const s: string | null = v;
    s.length = "x";
}

export function main(): string {
    // The members agree on `[number, string]`, so the tuple is inferred at it
    // and adds nothing: one error, not three.
    const u = ab(true);
    u.pair = [2, "y"];

    // Same receiver, a value that really is wrong. The agreed hint is what keeps
    // this error accurate rather than reporting it against a synthesized array.
    const v = ab(true);
    v.pair = [2, 2];

    // A nullable receiver: the field still has a type on what remains, so the
    // tuple is checked against it and stays silent.
    const w = maybeA(true);
    w.pair = [2, "y"];

    // One member readonly: the writable members still agree on a shape, so the
    // tuple types and the cast to the writable member is still offered.
    const m = mixed(true);
    m.pair = [3, "c"];

    const k = mixed(true);
    k.count += 1;

    const j = mixed(true);
    j.count++;

    // No write is legal at all, and the tuple still types against the shape:
    // one error, not three.
    const ro = maybeRO(true);
    ro.pair = [4, "d"];

    // The same write, with and without the `null`: both check the value.
    plainString("s");
    nullableString("s");

    // Not hint-driven, and not the receiver's fault.
    const z = ab(true);
    z.pair = notDefinedAnywhere;

    return "x";
}
