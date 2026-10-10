interface Sink<T> { emit(x: string): T; }
class Box<T> { constructor(public value: T) {} }
function identity<T>(value: T): T { return value; }
function withTiming<T>(f: () => T): T { return f(); }
function drain<T>(s: Sink<T>): T { return s.emit("z"); }
function main(): void {
  let calls = 0;
  withTiming<void>(() => { calls++; });
  withTiming(() => { calls++; });
  const sink: Sink<void> = { emit: (x: string): void => { calls += x.length; } };
  sink.emit("abc");
  identity<Sink<void>>(sink).emit("x");
  const box: Box<Sink<void>> = new Box<Sink<void>>(sink);
  box.value.emit("y");
  drain(sink);
  assert(calls === 8, "return-only void arguments");
}
