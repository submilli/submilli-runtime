// @target: es2015
// bug 819721
let r1 = null + null;
let r2 = null + undefined;
let r3 = undefined + null;
let r4 = undefined + undefined;

function main(): void {}
