// @target: es2015
class C {
    static foo(): void {
        C.foo = () => { }
    }

    static bar(x: number): number {
        C.bar = () => { } // error
        /*pruned*/;       // ok
        C.bar = (x: number) => 1; // ok
        return 1;
    }
}

function main(): void {}
