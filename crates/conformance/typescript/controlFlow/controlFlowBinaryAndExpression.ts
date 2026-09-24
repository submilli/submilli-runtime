// @target: es2015
let x: string | number | boolean = null as unknown as (string | number | boolean);
let cond: boolean = null as unknown as (boolean);

(x = "") && (x = 0);
x; // string | number

x = "";
cond && (x = 0);
x; // string | number


function main(): void {}
