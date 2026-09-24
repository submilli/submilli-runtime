//@target: ES5, ES2015
let union: string | number[] = null as unknown as (string | number[]);
for (let v of union) { }

function main(): void {}
