// @target: es2015
class Base {
    foo(): number { return 1; }
    static create(): Base {
        return new this();
    }
}

class Derived extends Base {
    foo(): number { return 2; }
}
let d = Derived.create(); 

d.foo();  

function main(): void {}
