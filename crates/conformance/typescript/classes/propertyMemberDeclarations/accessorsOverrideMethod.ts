// @target: esnext
// @useDefineForClassFields: true
class A {
    m(): void { }
}
class B extends A {
    get m(): () => number { return () => 1 }
}


function main(): void {}
