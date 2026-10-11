function nothing(): void {}
function maybe(): number | null { return null; }
function main(): void {
  let rejected = false;
  try { const value = (true ? nothing() : 1) as number; }
  catch (error) { rejected = true; }
  assert(rejected, "casting undefined to number is checked at runtime");
  assert(JSON.stringify({ value: maybe() ?? nothing() }) === "{}", "void field is omitted from JSON");
}
