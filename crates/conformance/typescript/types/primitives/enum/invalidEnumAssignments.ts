// @target: es2015
enum E {
    A,
    B
}

enum E2 {
    A,
    B
}

let e: E = null as unknown as (E);
let e2: E2 = null as unknown as (E2);

e = E2.A;
e2 = E.A;
e = <void>null;
e = {};
e = '';

function f<T>(a: T): void {
    e = a;
}

function main(): void {}
