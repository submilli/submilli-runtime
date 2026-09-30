// @target: es2015
class C {
    constructor(x: number) { }
    static foo: number;
    static bar(): typeof C {
        // type of this is the constructor function type
        let t = this;
        return this;
    }
}

let t = C.bar();
// all ok
let r2 = t.foo + 1;
let r3 = t.bar();
let r4 = new t(1);

class C2<T> {
    static test: number;
    constructor(x: string) { }
    static foo: string;
    static bar(): typeof C2 {
        // type of this is the constructor function type
        let t = this;
        return this;
    }
}

let t2 = C2.bar();
// all ok
let r5 = t2.foo + 1;
let r6 = t2.bar();
let r7 = new t2('');



function main(): void {}
