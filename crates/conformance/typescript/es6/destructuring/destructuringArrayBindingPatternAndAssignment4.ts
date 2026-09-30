// #35497

// @target: es5, es2015
// @downlevelIteration: true
// @lib: es6
// @strict: true

const data: number[] | null = null as unknown as (number[] | null);
const [value] = data; // Error


function main(): void {}
