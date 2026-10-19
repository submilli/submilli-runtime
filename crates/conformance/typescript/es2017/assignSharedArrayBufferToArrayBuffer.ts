// @target: es5, es2015
// @lib: es2015,es2017.sharedmemory

let foo: ArrayBuffer = new SharedArrayBuffer(1024); // should error

function main(): void {}
