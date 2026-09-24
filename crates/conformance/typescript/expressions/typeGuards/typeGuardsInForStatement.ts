// @target: es2015
let cond: boolean = null as unknown as (boolean);
function a(x: string | number): void {
    for (x = null; typeof x !== "number"; x = null) {
        x; // string
    }
    x; // number
}
function b(x: string | number): void {
    for (x = null; typeof x !== "number"; x = null) {
        x; // string
        if (cond) continue;
    }
    x; // number
}
function c(x: string | number): void {
    for (x = null; typeof x !== "number"; x = null) {
        x; // string
        if (cond) break;
    }
    x; // string | number
}


function main(): void {}
