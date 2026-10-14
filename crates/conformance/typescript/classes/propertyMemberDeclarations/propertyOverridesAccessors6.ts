// @strict: true
// @target: esnext
// @useDefineForClassFields: true
// @noEmit: true

class A {
  get x(): number {
    return 2;
  }
}
class B extends A {}
class C extends B {
  x: number = 1;
}


function main(): void {}
