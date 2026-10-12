// @target: es2015
enum E { A }
let n: number = null as unknown as (number);
let e: E = null as unknown as (E);
e = n;
n = e;

function main(): void {}
