// An assignment always passes exactly one value, so a setter's parameter can't
// be optional (tsc: TS1051), have a default (TS1052), or be a rest parameter
// (TS1053).
// expect-error-count: 3
// expect-error: a setter's parameter can't be optional
// expect-error: a setter's parameter can't have a default value
// expect-error: a setter's parameter can't be a rest parameter
class C {
  private v: number = 0;
  set a(x?: number) {}
  set b(x: number = 1) {}
  set c(...x: number[]) {}
  set d(x: number) { this.v = x; }
}

function main(): void {}
