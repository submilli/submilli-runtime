// A class can't extend one declared further down its module: the `extends`
// clause reads the parent before its declaration has run (tsc: TS2449).
// expect-error: class `Later` used before its declaration
// expect-error-count: 1
class Sub extends Later {}
class Later { x: number = 1; }

function main(): void {
  console.log(new Sub().x);
}
