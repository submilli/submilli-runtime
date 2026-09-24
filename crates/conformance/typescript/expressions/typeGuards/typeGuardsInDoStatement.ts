// @target: es2015
let cond: boolean = null as unknown as (boolean);
function a(x: string | number | boolean): void {
    x = true;
    do {
        x; // boolean | string
        x = null;
    } while (typeof x === "string")
    x; // number | boolean
}
function b(x: string | number | boolean): void {
    x = true;
    do {
        x; // boolean | string
        if (cond) continue;
        x = null;
    } while (typeof x === "string")
    x; // number | boolean
}
function c(x: string | number): void {
    x = "";
    do {
        x; // string
        if (cond) break;
        x = null;
    } while (typeof x === "string")
    x; // string | number
}


function main(): void {}
