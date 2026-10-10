// Type dispatch must peel aliases. Sites that match a `Type` to pick a
// *lowering* are the dangerous ones: the scalar arms of JSON stringification
// lower unboxed, so an alias falling through to the ref-taking default emits an
// f64/i32 where a ref is expected — an invalid module rather than a diagnostic.
// The narrowing helpers are the same class: an unpeeled match silently declines
// to narrow instead of miscompiling.
type Num = number;
type Bool = boolean;
type Nul = null;
type Str = string;
type NN = number | null;
type Tag = "a";

type Tagged = { kind: Tag; x: number };
type Other = { kind: "b"; y: number };

interface Holder {
  n: Num;
  b: Bool;
}

function main(): void {
  // JSON of aliased scalars — each of these lowers unboxed
  const n: Num = 4;
  const b: Bool = true;
  const x: Nul = null;
  const s: Str = "hi";
  assert(JSON.stringify(n) === "4", "aliased number");
  assert(JSON.stringify(b) === "true", "aliased boolean");
  assert(JSON.stringify(x) === "null", "aliased null");
  assert(JSON.stringify(s) === "\"hi\"", "aliased string");

  // through an interface field, so the alias arrives from a member type
  const h: Holder = { n: 7, b: false };
  assert(JSON.stringify(h.n) === "7", "aliased number through a field");
  assert(JSON.stringify(h.b) === "false", "aliased boolean through a field");

  // narrowing strips null through an alias of a union
  const maybe: NN = 5;
  if (maybe !== null) {
    assert(maybe.toString() === "5", "alias of a nullable union narrows");
  } else {
    assert(false, "maybe is non-null");
  }

  // ...and when the `null` member itself is an alias
  const viaAliasNull: number | Nul = 6;
  if (viaAliasNull !== null) {
    assert(viaAliasNull.toString() === "6", "aliased null member is stripped");
  } else {
    assert(false, "viaAliasNull is non-null");
  }

  // a discriminant whose literal type is reached through an alias
  const w: Tagged | Other = { kind: "a", x: 3 };
  if (w.kind === "a") {
    assert(w.x === 3, "aliased literal tag discriminates");
  } else {
    assert(false, "w is the tagged variant");
  }
}
