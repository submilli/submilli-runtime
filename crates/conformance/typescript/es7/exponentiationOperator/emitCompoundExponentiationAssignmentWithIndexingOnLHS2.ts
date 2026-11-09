// @target: es5, es2015
let globalCounter = 0;
function foo(): { 0: number; } {
    globalCounter += 1;
    return { 0: 2 };
}
foo()[0] **= foo()[0];
let result_foo1 = foo()[0] **= foo()[0];
foo()[0] **= foo()[0] **= 2;
let result_foo2 = foo()[0] **= foo()[0] **= 2;
foo()[0] **= foo()[0] ** 2;
let result_foo3 = foo()[0] **= foo()[0] ** 2;

function main(): void {}
