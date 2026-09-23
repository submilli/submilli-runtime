// @target: es2015
// @declaration: true

function myRandBool(): boolean { return null as unknown as (boolean); }

let a: "foo" = ("foo");
let b: "foo" | "bar" = ("foo");
let c: "foo" = (myRandBool ? "foo" : ("foo"));
let d: "foo" | "bar" = (myRandBool ? "foo" : ("bar"));


function main(): void {}
