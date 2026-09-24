// @target: es2015
// @strict: true

const f11: 1 | 0 | '' | null | null = null as unknown as (1 | 0 | '' | null | null);

let g11 = f11 ?? f11.toFixed()




function main(): void {}
