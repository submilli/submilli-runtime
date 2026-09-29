// @target: es6
// @lib: es2017

let foge = new SharedArrayBuffer(1024);
let bar = foge.slice(1, 10);
let stringTag = foge[Symbol.toStringTag];
let len = foge.byteLength;
let species = SharedArrayBuffer[Symbol.species];

function main(): void {}
