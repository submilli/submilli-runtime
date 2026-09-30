// @target: es5, es2015
// @lib: es5,es2017.sharedmemory

let foge = new SharedArrayBuffer(1024);
let bar = foge.slice(1, 10);
let len = foge.byteLength;

function main(): void {}
