// @target: es2015
let x: "foo" = null as unknown as ("foo");
let y: "foo" | "bar" = null as unknown as ("foo" | "bar");

let b: boolean = null as unknown as (boolean);
b = x === y;
b = "foo" === y
b = y === "foo";
b = "foo" === "bar";
b = "bar" === x;
b = x === "bar";
b = y === "bar";
b = "bar" === y;

b = x !== y;
b = "foo" !== y
b = y !== "foo";
b = "foo" !== "bar";
b = "bar" !== x;
b = x !== "bar";
b = y !== "bar";
b = "bar" !== y;



function main(): void {}
