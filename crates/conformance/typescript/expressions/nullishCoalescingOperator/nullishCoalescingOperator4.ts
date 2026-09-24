// @target: es2015
// @strict: true

const a1: 'literal' | null | null = null as unknown as ('literal' | null | null);
const aa1 = a1 ?? a1.toLowerCase()
const aa2 = a1 || a1.toLocaleUpperCase()


function main(): void {}
