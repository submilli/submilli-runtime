// @target: es2015
function foo3(cb: (x: number) => number): typeof cb { return null as unknown as (typeof cb); }
let r5 = foo3((x: number) => ''); // error

function main(): void {}
