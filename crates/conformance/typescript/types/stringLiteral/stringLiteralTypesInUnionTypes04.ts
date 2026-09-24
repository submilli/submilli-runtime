// @target: es2015
// @strict: true
// @declaration: true

type T = "" | "foo";

let x: T = null;
let y: T = null;

if (x === "") {
    let a = x;
}

if (x !== "") {
    let b = x;
}

if (x == "") {
    let c = x;
}

if (x != "") {
    let d = x;
}

if (x) {
    let e = x;
}

if (!x) {
    let f = x;
}

if (!!x) {
    let g = x;
}

if (!!!x) {
    let h = x;
}

function main(): void {}
