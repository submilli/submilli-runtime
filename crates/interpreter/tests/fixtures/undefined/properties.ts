type Optional = { a?: string };
type Required = { a: string | undefined };
function main(): void {
  const omitted: Optional = {};
  const present: Optional = { a: undefined };
  const required: Required = { a: undefined };
  assert(omitted.a === undefined);
  assert(present.a === undefined && required.a === undefined);
  assert(!("a" in omitted), "omitted field stays absent");
  assert("a" in present, "explicit undefined stays present");
  assert(Object.keys(omitted).length === 0);
  assert(Object.keys(present).length === 1);
  present.a = undefined;
  assert("a" in present, "undefined write preserves presence");
  const spread = { ...present };
  assert("a" in spread && spread.a === undefined);
  assert(JSON.stringify(present) === "{}");
  assert((omitted.a ?? "fallback") === "fallback");
}
