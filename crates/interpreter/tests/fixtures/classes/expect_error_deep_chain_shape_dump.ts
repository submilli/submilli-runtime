// Three levels: each member is attributed to the class that declares it, and an
// override is attributed to the overriding class, not the one it shadows.
// expect-error: field `nope` does not exist on `C`
// expect-error: class C extends B
// expect-error: static kind: string;  // from A
// expect-error: a: number;  // from A
// expect-error: b: string;  // from B
// expect-error: shared(): string;  // from B
// expect-error: onlyA(): number;  // from A
class A {
  static kind: string = "a";
  a: number = 1;
  shared(): string {
    return "A";
  }
  onlyA(): number {
    return 1;
  }
}

class B extends A {
  b: string = "b";
  shared(): string {
    return "B";
  }
}

class C extends B {}

function main(): void {
  const c = new C();
  console.log(`${c.nope}`);
}
