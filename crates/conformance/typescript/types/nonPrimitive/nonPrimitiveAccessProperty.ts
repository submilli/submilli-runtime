// @target: es2015
let a: object = {};
a.toString();
a.nonExist(); // error

let { destructuring } = a; // error
let { ...rest } = a; // ok


function main(): void {}
