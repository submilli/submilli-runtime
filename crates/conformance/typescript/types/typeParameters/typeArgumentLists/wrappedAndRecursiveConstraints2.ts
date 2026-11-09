// @target: es2015
class C<T extends C<T>> { // error
    constructor(x: T) { }
}

let c = new C(1);
let c_2 = new C(new C('')); // error

function main(): void {}
