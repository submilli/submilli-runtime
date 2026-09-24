// @target: es2015
let x: string | number | boolean = null as unknown as (string | number | boolean);
let cond: boolean = null as unknown as (boolean);

cond ? x = "" : x = 3;
x; // string | number


function main(): void {}
