// A keyword is a member name before `:`, `?` or `(`; only `keyword name` starts a
// declaration.
type A = { function: number; class?: string; let(): number; type: number };
interface I { const: number
  enum: string }
function main(): void {
  const a: A = { function: 1, let: (): number => 2, type: 3 };
  const i: I = { const: 4, enum: "e" };
  assert(a.function + a.let() + a.type + i.const === 10, "keyword-named members");
  assert(i.enum === "e", "keyword-named member on a new line");
}
