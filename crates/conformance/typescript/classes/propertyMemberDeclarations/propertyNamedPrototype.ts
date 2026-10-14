// @target: es2015
class C {
    prototype: number; // ok
    static prototype: C; // error
}

function main(): void {}
