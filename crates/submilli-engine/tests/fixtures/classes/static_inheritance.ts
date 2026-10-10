class A {
  static readonly X: number = 10;
  static f(): number {
    return 1;
  }
}

class B extends A {}

class C extends B {
  static g(): number {
    return B.f() + 2;
  }
}

function main(): void {
  assert(B.f() === 1);
  assert(B.X === 10);
  assert(C.g() === 3);
  assert(C.X === 10);
}
