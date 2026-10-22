// An optional chain ending in a property a host interface implements narrows
// its receivers, and reads the property again rather than narrowing it.
function f(m: Set<string> | undefined, n: number): number {
  if (m?.size === n) return m.size;
  return -1;
}
function g(m: Map<string, number> | undefined): number {
  if (typeof m?.size === "number") return m.size;
  return -1;
}
function h(r: RegExp | undefined, s: string): string {
  if (r?.source === s) return r.source;
  return "";
}
function main(): void {
  assert(f(new Set(["a"]), 1) === 1);
  assert(f(undefined, 1) === -1);
  assert(g(new Map()) === 0);
  assert(h(/ab/, "ab") === "ab");
}
