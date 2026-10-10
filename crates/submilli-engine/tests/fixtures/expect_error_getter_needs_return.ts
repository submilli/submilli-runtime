// expect-error: getter `C.u` has no `return`; a getter must return a value
// expect-error: getter `C.v` has no `return`; a getter must return a value
// expect-error-count: 2
// A getter must return something whatever its declared type (TypeScript's
// TS2378); `return undefined;` says so explicitly.
class C {
  get u(): undefined {
    console.log("u");
  }
  get v(): void {
    console.log("v");
  }
  get w(): undefined {
    return undefined;
  }
}
function main(): void {
  console.log(String(new C().w));
}
