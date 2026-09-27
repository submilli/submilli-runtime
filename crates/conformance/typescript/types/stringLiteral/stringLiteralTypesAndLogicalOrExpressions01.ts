// @target: es2015
// @declaration: true

function myRandBool(): boolean { return null as unknown as (boolean); }

let a: "foo" = "foo";
let b = a || "foo";
let c: "foo" = b;
let d = b || "bar";
let e: "foo" | "bar" = d;


function main(): void {}
