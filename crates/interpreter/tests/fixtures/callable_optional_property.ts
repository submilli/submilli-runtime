interface Labeled { (n: number): number; label?: string; count?: number; length?: number; }
function check(f: Labeled): void {
  assert(f.label === undefined);
  assert(f.count === undefined);
  assert(f?.label === undefined);
  assert(f(2) === 3);
  assert(f.length === 1 && f?.length === 1);
}
function main(): void {
  check((n) => n + 1);
  const hybrid: Labeled = Object.assign((n: number) => n + 1, { label: "ok", count: 4 });
  assert(hybrid.label === "ok" && hybrid.count === 4);
  assert(hybrid?.label === "ok");
  assert(hybrid.length === 1 && hybrid?.length === 1);
}
