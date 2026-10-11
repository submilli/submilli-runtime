// @target: es2015
// @strict: true
// @declaration: true

type T = string | "foo" | "bar" | "baz";

let x: "foo" | "bar" | "baz" | string = undefined;
let y: T = undefined;

if (x === "foo") {
    let a = x;
}
else if (x !== "bar") {
    let b = x || y;
}
else {
    let c = x;
    let d = y;
    let e: (typeof x) | (typeof y) = c || d;
}

x = y;
y = x;

function main(): void {}
