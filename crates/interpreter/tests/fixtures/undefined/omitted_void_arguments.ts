// A trailing parameter whose type accepts `void` may be left out, as in
// TypeScript: the call passes `undefined`.
function c(x: number | void, y: void, z: void | string | number): string {
  return `${x === undefined} ${y === undefined} ${z === undefined}`;
}

class Box<T> {
  put(a: number, b: T): boolean {
    return b === undefined;
  }
}

class Deferred<X> {
  settled: boolean = false;
  constructor(executor: (resolve: (value: X) => void) => void) {
    executor((_value: X) => {
      this.settled = true;
    });
  }
}

function main(): void {
  assert(c(3, undefined, "s") === "false true false", "every argument passed");
  assert(c(3) === "false true true", "trailing void arguments omitted");
  assert(c() === "true true true", "all arguments omitted");
  const late: (a: number, b: void) => boolean = (_a: number, b: void) => b === undefined;
  assert(late(1), "a function value's void parameter omitted");
  assert(new Box<void>().put(2), "a receiver's void type argument makes the parameter omittable");
  assert(new Deferred<void>(resolve => resolve()).settled, "a void callback parameter omitted");
  assert(new Deferred<void | number>(resolve => resolve()).settled, "a union with void omitted");
}
