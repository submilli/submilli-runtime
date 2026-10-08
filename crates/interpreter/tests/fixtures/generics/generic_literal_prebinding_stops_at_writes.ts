// Before typing a callback inside a literal argument, the parts of the call
// that come before the first write bind its type parameters early. A part that
// assigns, or calls a function literal on the spot, is left to be inferred in
// turn, so the code before it sees the narrowings in effect there, as tsc
// types it. Nested generic calls with callbacks infer in time linear in their
// depth.
function f<T>(o: { before: number; cb: (t: T) => void; after: T }): number {
  o.cb(o.after);
  return o.before;
}

function k<T>(o: { cb: (t: T) => void }, n: number, v: T): number {
  o.cb(v);
  return n;
}

function nest<T>(o: { cb: (x: T) => T; v: T }): T {
  return o.cb(o.v);
}

function main(): void {
  let x: string | number = "str";
  const seen: string[] = [];
  const before = f({ before: x.length, cb: (t) => { seen.push(String(t)); }, after: (x = 5) });
  assert(before === 3 && x === 5, "an assigning field");

  let y: string | number = "four";
  const n = k({ cb: (t) => { seen.push(String(t)); } }, y.length, (() => { y = 5; return 0; })());
  assert(n === 4 && y === 5, "a function literal called on the spot");
  assert(seen.join(",") === "5,0", "callbacks see the written values");

  const deep = nest({ cb: (a) => a + 1, v: nest({ cb: (b) => b + 1, v: nest({ cb: (c) => c + 1, v: nest({ cb: (d) => d + 1, v: nest({ cb: (e) => e + 1, v: nest({ cb: (g) => g + 1, v: nest({ cb: (h) => h + 1, v: nest({ cb: (i) => i + 1, v: nest({ cb: (j) => j + 1, v: nest({ cb: (l) => l + 1, v: nest({ cb: (m) => m + 1, v: nest({ cb: (q) => q + 1, v: 0 }) }) }) }) }) }) }) }) }) }) }) });
  assert(deep === 12, "nested generic calls");
}
