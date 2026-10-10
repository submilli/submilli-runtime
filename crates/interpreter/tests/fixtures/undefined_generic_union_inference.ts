interface Data { name: string }
function required<T>(value: T | null | undefined): T {
  if (value === null || value === undefined) { throw new Error("missing value"); }
  return value;
}
function read(value: Data | undefined): string {
  const data = required(value);
  return data.name;
}
function main(): void {
  assert(read({ name: "present" }) === "present", "infer T after excluding fixed nullish arms");
  let rejected = false;
  try { read(undefined); } catch (error) { rejected = true; }
  assert(rejected, "inference preserves nullish runtime guard");
}
