// @target: es2015
let o: object = null as unknown as (object);
const x: { a: number, b: number } = { a: 1, ...o, b: 2 };


function main(): void {}
