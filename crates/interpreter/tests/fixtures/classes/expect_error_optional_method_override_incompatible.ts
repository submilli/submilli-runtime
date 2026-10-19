// expect-error: method `f` is not compatible with the inherited optional method
class Base {
  f?(x: number): number;
}
class Derived extends Base {
  f(x: string): number {
    return x.length;
  }
}
function main(): void {}
