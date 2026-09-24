// @target: es2015
let fooOrBar: "foo" | "bar" = null as unknown as ("foo" | "bar");

let a = "foo" as "bar";
let b = "bar" as "foo";
let c = fooOrBar as "foo";
let d = fooOrBar as "bar";
let e = fooOrBar as "baz";
let f = "baz" as typeof fooOrBar;

function main(): void {}
