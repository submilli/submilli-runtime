// A `$string` built entirely host-side (the migrated `number.toString` returns
// a real `$string`, not a raw array re-wrapped by a prelude shim) must behave
// as a full `$Object` subtype: canonicalize with module `$string`, and dispatch
// its vtable for equality, hashing (Map keys), and JSON serialization.
function main(): void {
  const s: string = (42).toString();

  // Structural equality against a literal `$string`.
  assert(s === "42", "host-built string equals a literal");

  // Concat copies over the host-built payload.
  assert(s + "!" === "42!", "host-built string concatenates");

  // Map key — exercises the string vtable's hash + equals on a host-built value.
  const m = new Map<string, number>();
  m.set(s, 7);
  assert(m.has("42"), "host-built string is found as a Map key");
  assert(m.get("42") === 7, "host-built string Map lookup returns its value");

  // JSON.stringify dispatches the string vtable's toJson on the host-built value.
  assert(JSON.stringify(s) === "\"42\"", "host-built string serializes via its vtable");
}
