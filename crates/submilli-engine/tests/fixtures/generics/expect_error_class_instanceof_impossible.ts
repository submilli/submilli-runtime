// expect-error: is always false
// `StrBox` fixes its parent's argument to `string`, so no `Box<number>` can
// ever be one — the erased runtime walk must not hide that.
class Box<T> {
  constructor(public v: T) {}
}

class StrBox extends Box<string> {
  s: number = 1;
}

function f(x: Box<number>): number {
  if (x instanceof StrBox) {
    return x.s;
  }
  return 0;
}

function main(): void {
  assert(f(new Box<number>(1)) === 0);
}
