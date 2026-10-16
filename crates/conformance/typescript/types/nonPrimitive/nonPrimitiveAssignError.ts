// @target: es2015
let x = {};
let y = {foo: "bar"};
let a: object = {};
x = a;
y = a; // expect error
a = x;
a = y;

let n = 123;
let b = true;
let s = "fooo";

a = n; // expect error
a = b; // expect error
a = s; // expect error

n = a; // expect error
b = a; // expect error
s = a; // expect error

let numObj: Number = 123;
let boolObj: Boolean = true;
let strObj: String = "string";

a = numObj; // ok
a = boolObj; // ok
a = strObj; // ok


function main(): void {}
