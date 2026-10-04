// A literal-union or `boolean` tag rules out union members the same way a
// single literal does, several tags narrow in turn, and a member without the
// tag field stays, so each literal below names a field only a ruled-out member
// has. When a tag fits no member that declares it, a member without the field
// is left, and the tag itself is unknown to it (`noDeclaringFit`; tsc rejects
// that literal as a whole). The other literals' tags also widen, which is
// reported separately.
// expect-error: object literal has unknown field `c` for the target type
// expect-error: object literal has unknown field `b` for the target type
// expect-error: valid fields: `a`, `c`, `k`
// expect-error: object literal has unknown field `x` for the target type
// expect-error: object literal has unknown field `k` for the target type
// expect-error: valid fields: `c`
type ByTags = { k: "x" | "y"; b: number } | { k: "z"; c: number } | { k: true; d: number };
type MaybeTagged = { k: "x"; a: number } | { k: "y"; b: number } | { c: number };
type TwoTags = { k: "a"; m: 1; x: number } | { k: "b"; m: 2; y: number } | { k: "a"; m: 2; z: number };
type OneTagged = { k: "x"; a: number } | { c: number };

function main(): void {
  const pickedByUnion: ByTags = { k: "y", b: 1, c: 2 };
  const pickedByBool: ByTags = { k: true, d: 1, b: 2 };
  const keptWithoutTag: MaybeTagged = { k: "x", a: 1, b: 2 };
  const pickedByBoth: TwoTags = { k: "a", m: 2, z: 1, x: 1 };
  const noDeclaringFit: OneTagged = { k: "y", c: 2 };
}
