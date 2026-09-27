// expect-error: `this` is only valid inside a class method or constructor body
// expect-error-count: 1
class C {
  x: number = 1;
  m(): number {
    const o = { x: 2, g(): number { return this.x; } };
    return o.g();
  }
}
function main(): void {}
