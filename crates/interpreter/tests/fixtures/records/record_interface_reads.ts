interface Values { [key: string]: number; }
interface NamedValues { [key: string]: number; x: number; }
function dot(values: Values): number | null { return values.x; }
function literal(values: Values): number | null { return values["x"]; }
function optional(values: Values | null): number | null { return values?.x; }
function optionalLiteral(values: Values | null): number | null { return values?.["x"]; }
function named(values: NamedValues): number { return values.x; }
function main(): void {
  const values: NamedValues = { x: 1 };
  assert(dot(values) === 1);
  assert(literal(values) === 1);
  assert(optional(values) === 1);
  assert(optionalLiteral(values) === 1);
  assert(optional(null) === null);
  assert(optionalLiteral({}) === null);
  const wide: Record<string, unknown> = values;
  wide["x"] = "wrong";
  let errors = 0;
  try { dot(values); } catch (e: Error) { assert(e.name === "TypeError"); errors += 1; }
  try { literal(values); } catch (e: Error) { assert(e.name === "TypeError"); errors += 1; }
  try { optional(values); } catch (e: Error) { assert(e.name === "TypeError"); errors += 1; }
  try { optionalLiteral(values); } catch (e: Error) { assert(e.name === "TypeError"); errors += 1; }
  try { named(values); } catch (e: Error) { assert(e.name === "TypeError"); errors += 1; }
  assert(errors === 5);
}
