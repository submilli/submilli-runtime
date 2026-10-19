// @target: esnext, es2022, es6, es5

class C {
    static create: () => C = () => new this("yep")

    constructor (private foo: string) {

    }
}


function main(): void {}
