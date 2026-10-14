// @target: es2015
let a: number[] = null as unknown as (number[]);
let [...x = a] = a;  // Error, rest element cannot have initializer


function main(): void {}
