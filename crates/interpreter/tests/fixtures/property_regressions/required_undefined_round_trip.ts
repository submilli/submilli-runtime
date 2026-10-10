// JSON has no `undefined`, so `JSON.stringify` leaves the field out. A required
// field whose type includes `undefined` still accepts that missing key on the
// way back, since reading it yields `undefined` either way.
interface Node { id: number; parent: number | undefined; }
function main(): void {
  const text = JSON.stringify({ id: 1, parent: undefined });
  assert(text === '{"id":1}', "undefined fields are omitted");
  const node = JSON.parse(text) as Node;
  assert(node.parent === undefined, "a missing key reads as undefined");
  const nested = JSON.parse('{"id":2,"parent":3}') as Node;
  assert(nested.parent === 3, "a present value is still checked and kept");
  let rejected = false;
  try { const bad = JSON.parse('{"id":2,"parent":null}') as Node; } catch (e) { rejected = e instanceof TypeError; }
  assert(rejected, "null is not undefined");
}
