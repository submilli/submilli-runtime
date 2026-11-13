// expect-error: unsupported Record key type
// expect-error: expects two type arguments
// expect-error: does not satisfy index signature
// expect-error: cannot assign through readonly index signature
// expect-error: expected
interface BadMethods {
  [key: string]: number;
  read(): number;
}
function main(): void {
  const invalid: Record<{ id: number }, string> = {};
  const arity: Record<string> = {};
  const mixed: { [key: string]: number; label: string } = { label: "wrong" };
  const fixed: { readonly [key: string]: number } = {};
  const key: string = "x";
  fixed[key] = 1;
  const record: Record<string, number> = {};
  record[key] = "wrong";
}
