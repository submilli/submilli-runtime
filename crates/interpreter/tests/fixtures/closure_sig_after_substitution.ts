// A generic member's closure shape is only known after substitution: `void`
// changes the closure ABI (no result slot), so `Box<void>` needs a sig the
// declaration `(x: string) => T` does not name. Only the `number` instantiation
// is ever built, so nothing registers the `void` one by accident.
//
// The method-syntax half is here for the pairing, not for coverage — its sig is
// rebuilt at the call site, and `closure_sig_shape_dispatch` is what pins that.
interface Box<T> {
  run: (x: string) => T;
}

interface Sink<T> {
  emit(x: string): T;
}

function callNum(b: Box<number>): number {
  return b.run("a");
}

function callVoid(b: Box<void>): void {
  b.run("a");
}

function sinkNum(s: Sink<number>): number {
  return s.emit("a");
}

function sinkVoid(s: Sink<void>): void {
  s.emit("a");
}

function main(): void {
  const nums: Box<number> = { run: (x: string): number => x.length };
  assert(callNum(nums) === 1, "property member, number instantiation");

  const sink: Sink<number> = { emit: (x: string): number => x.length + 1 };
  assert(sinkNum(sink) === 2, "method member, number instantiation");
}
