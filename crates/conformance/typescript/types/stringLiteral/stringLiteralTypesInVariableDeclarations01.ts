// @target: es2015
// @strict: true
// @declaration: true

let a: "" = null as unknown as ("");
let b: "foo" = null as unknown as ("foo");
let c: "bar" = null as unknown as ("bar");
const d: "baz" = null as unknown as ("baz");

a = "";
b = "foo";
c = "bar";

let e: "" = "";
let f: "foo" = "foo";
let g: "bar" = "bar";
const h: "baz" = "baz";

e = "";
f = "foo";
g = "bar";

function main(): void {}
