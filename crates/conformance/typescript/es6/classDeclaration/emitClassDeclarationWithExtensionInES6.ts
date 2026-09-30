// @target: es6
class B {
    baz(a: string, y: number = 10): void { }
}
class C extends B {
    foo(): void { }
    baz(a: string, y:number): void {
        super.baz(a, y);
    }
}
class D extends C {
    constructor() {
        super();
    }

    foo(): void {
        super.foo();
    }

    baz(): void {
        super.baz("hello", 10);
    }
}


function main(): void {}
