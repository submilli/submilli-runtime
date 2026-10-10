// A function declared in a method has no receiver; TypeScript rejects `this`
// there too.
// expect-error: `this` is only valid inside a class method or constructor body
// expect-error-count: 1
class C {
  n: number = 1;
  m(): number {
    function g(): number {
      return this.n;
    }
    return g();
  }
}
function main(): void {}
