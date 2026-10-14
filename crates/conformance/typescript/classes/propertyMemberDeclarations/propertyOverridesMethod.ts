// @target: esnext
// @useDefineForClassFields: true
class A {
    m(): void { }
}
class B extends A {
    m: () => number = () => 1
}


function main(): void {}
