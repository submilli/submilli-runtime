import session from "submilli:session";

interface Progress { rows: { count: number }[]; }
type Link = { value: number; next: Link | null };
let keys = 0;
function key(): string { keys += 1; return "tenant/" + "progress"; }

function main(): void {
  const rows: { count: number | string }[] = [{ count: 1 }, { count: "PRIVATE" }];
  session.set("tenant/progress", { rows });
  let caught = false;
  try { session.get<Progress>(key()); }
  catch (e: Error) {
    caught = true;
    assert(e instanceof TypeError, "checked casts still throw TypeError");
    assert(e.message.includes('$["rows"][1]["count"]'), e.message);
    assert(e.message.includes("tenant/progress"), "dynamic key is named");
    assert(!e.message.includes("PRIVATE"), "stored values stay private");
  }
  assert(caught && keys === 1, "key evaluated once");
  const recursive: unknown = { value: 1, next: { value: "PRIVATE", next: null } };
  caught = false;
  try { const result = recursive as Link; }
  catch (e: Error) {
    caught = true;
    assert(e.message.includes('$["next"]["value"]'), e.message);
    assert(!e.message.includes("PRIVATE"), "recursive values stay private");
  }
  assert(caught, "recursive mismatch rejected");
  const union: unknown = { a: "yes", z: false };
  caught = false;
  try { const result = union as { a: number | string; z: number }; }
  catch (e: Error) {
    caught = true;
    assert(e.message.includes('$["z"]'), e.message);
  }
  assert(caught, "union success discards failed-arm diagnostics");
  const nullable: unknown = { foo: { bar: { n: "PRIVATE" } } };
  caught = false;
  try { const result = nullable as { foo: { bar: { n: number } } | null }; }
  catch (e: Error) {
    caught = true;
    assert(e.message.includes('$["foo"]["bar"]["n"]'), e.message);
  }
  assert(caught, "nullable alternatives retain the detailed failure");
  const pair: [number, string] = [1, "PRIVATE"];
  const tuple: unknown = pair;
  caught = false;
  try { const result = tuple as [number, number]; }
  catch (e: Error) { caught = true; assert(e.message.includes("$[1]"), e.message); }
  assert(caught, "tuple mismatch reports index");
}
