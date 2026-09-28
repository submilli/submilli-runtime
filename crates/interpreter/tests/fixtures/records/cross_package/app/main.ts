import { make, put, read, readOptional } from "@test/records";
function main(): void {
  const values = make();
  put(values, "new", 3);
  assert(values.new === 3);
  assert(read(values, "count") === 1);
  assert(read(values, "absent") === null);
  assert(JSON.stringify(values) === '{"count":1,"new":3}');
  assert(readOptional(values) === 1);
  const wide: Record<string, unknown> = values;
  wide["count"] = "wrong";
  let errors = 0;
  try { const count = values.count; } catch (e: Error) { assert(e.name === "TypeError"); errors += 1; }
  try { readOptional(values); } catch (e: Error) { assert(e.name === "TypeError"); errors += 1; }
  assert(errors === 2);
}
