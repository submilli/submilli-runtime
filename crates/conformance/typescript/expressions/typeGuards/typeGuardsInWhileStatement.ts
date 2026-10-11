// @target: es2015
let cond: boolean = null as unknown as (boolean);
function a(x: string | number): void {
    while (typeof x === "string") {
        x; // string
        x = undefined;
    }
    x; // number
}
function b(x: string | number): void {
    while (typeof x === "string") {
        if (cond) continue;
        x; // string
        x = undefined;
    }
    x; // number
}
function c(x: string | number): void {
    while (typeof x === "string") {
        if (cond) break;
        x; // string
        x = undefined;
    }
    x; // string | number
}


function main(): void {}
