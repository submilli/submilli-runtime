// expect-error: `f` redeclares an inherited field as a method
// A function-typed optional property is not an optional method: tsc rejects
// overriding it with a method too.
class Base {
  f?: () => number;
}
class Derived extends Base {
  f(): number {
    return 1;
  }
}
function main(): void {}
