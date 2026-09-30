// @target: es5, es2015

let globalCounter = 0;
function foo(): { prop: number; } {
    globalCounter += 1;
    return { prop: 2 };
}
foo().prop **= 2;
let result0 = foo().prop **= 2;
foo().prop **= foo().prop **= 2;
let result1 = foo().prop **= foo().prop **= 2;
foo().prop **= foo().prop ** 2;
let result2 = foo().prop **= foo().prop ** 2;

function main(): void {}
