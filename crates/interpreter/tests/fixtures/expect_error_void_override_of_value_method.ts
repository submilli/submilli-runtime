// A `void` function fits a function type returning `unknown`, but an override
// fills the inherited method's slot, whose callers read the value it returns,
// so a `void` override of a method returning `unknown` is rejected.
// expect-error: override of method `m` is not compatible with the inherited signature
// expect-error-count: 1
class A {
  m(): unknown {
    return 1;
  }
}

class B extends A {
  m(): void {}
}

function main(): void {
  const a: A = new B();
  a.m();
}
