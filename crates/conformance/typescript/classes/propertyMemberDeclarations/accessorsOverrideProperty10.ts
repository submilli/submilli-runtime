// @strict: true
// @target: esnext
// @useDefineForClassFields: true
// @noEmit: true

class A {
  x: number = 1;
}
class B extends A {}
class C extends B {
  get x(): number {
    return 2;
  }
}


function main(): void {}
