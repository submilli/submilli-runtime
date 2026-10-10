// TypeScript lets an override declare fewer parameters, as any function may.
// An override here fills the inherited method's fixed-arity vtable slot, so it
// must declare them all.
// expect-error: override of method `f` must declare the inherited method's 2 parameter(s)
// expect-error-count: 1
class A {
  f(x: number, y: number): number {
    return x + y;
  }
}
class B extends A {
  f(x: number): number {
    return x * 10;
  }
}
function main(): void {}
