// expect-error: expected `number`, got `number | string`
// expect-error-count: 1
// A destructured parameter narrows its siblings only while no write reaches
// it: after `kind` is reassigned, testing it says nothing of `payload`, as in
// tsc.
type Data = { kind: "num"; payload: number } | { kind: "str"; payload: string };
function f({ kind, payload }: Data, other: Data): number {
  kind = other.kind;
  if (kind === "num") {
    return payload;
  }
  return 0;
}
function main(): void {
  f({ kind: "num", payload: 1 }, { kind: "str", payload: "s" });
}
