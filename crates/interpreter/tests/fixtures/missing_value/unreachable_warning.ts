// SUB-2148: a `??` on a value that can't be null and that no missing read
// reaches still warns.
// expect-warning: `??` on non-nullable type `string` — right side is unreachable
function main(): void {
  const s: string = "x";
  assert((s ?? "d") === "x", "the left side decides");
}
