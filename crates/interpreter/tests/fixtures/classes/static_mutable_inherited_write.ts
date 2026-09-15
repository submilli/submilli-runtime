class A {
  static x: number = 1;
}

class B extends A {}

class C extends B {}

function main(): void {
  B.x = 7;
  // One slot, not one per subclass.
  assert(A.x === 7);
  assert(B.x === 7);
  assert(C.x === 7);

  C.x += 1;
  assert(A.x === 8);
}
