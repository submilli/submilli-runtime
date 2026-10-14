// @target: esnext
// @useDefineForClassFields: true
class A {
    get p(): string { return 'oh no' }
}
class B extends A {
    constructor(public p: string) {
        super()
    }
}


function main(): void {}
