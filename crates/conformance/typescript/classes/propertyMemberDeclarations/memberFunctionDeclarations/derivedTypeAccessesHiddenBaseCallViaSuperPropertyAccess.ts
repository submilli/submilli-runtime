// @target: es2015
class Base {
    foo(x: { a: number }): { a: number } {
        return null;
    }
}

class Derived extends Base {
    foo(x: { a: number; b: number }): { a: number; b: number } {
        return null;
    }

    bar(): void {
        let r = super.foo({ a: 1 }); // { a: number }
        let r2 = super.foo({ a: 1, b: 2 }); // { a: number }
        let r3 = this.foo({ a: 1, b: 2 }); // { a: number; b: number; }
    }
}

function main(): void {}
