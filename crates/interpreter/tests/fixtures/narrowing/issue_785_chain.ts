interface Inner { name: string; }
interface Outer { inner: Inner | null; }
function read(o: Outer | null): string {
  if (o?.inner !== undefined && o.inner !== null) { return o.inner.name; }
  return "-";
}
function reversed(o: Outer | null): string {
  if (undefined === o?.inner || null === o.inner) { return "-"; }
  return o.inner.name;
}
function truthy(o: Outer | null): string {
  if (o?.inner) { return o.inner.name; }
  return "-";
}
interface Root { outer: Outer | null; }
function deep(r: Root | null): string {
  if (r?.outer?.inner !== undefined && r.outer.inner !== null) { return r.outer.inner.name; }
  return "-";
}
export function main(): void {
  const full: Outer = { inner: { name: "yes" } };
  const empty: Outer = { inner: null };
  assert(read(full) === "yes", "field receiver and result narrow");
  assert(read(empty) === "-", "null leaf");
  assert(read(null) === "-", "null root");
  assert(reversed(full) === "yes", "reversed null guard after return");
  assert(reversed(null) === "-", "short circuit has no leaf fact");
  assert(truthy(full) === "yes", "truthy chain");
  assert(truthy(empty) === "-", "falsy leaf");
  assert(deep({ outer: full }) === "yes", "every optional prefix narrows");
  assert(deep({ outer: null }) === "-", "middle short circuit");
  assert(deep(null) === "-", "deep root short circuit");
}
