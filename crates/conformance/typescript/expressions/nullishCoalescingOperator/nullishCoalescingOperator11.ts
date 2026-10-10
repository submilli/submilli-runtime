// @target: es2015
// @strict: true

const f11: 1 | 0 | '' | null | undefined = null as unknown as (1 | 0 | '' | null | undefined);

let g11 = f11 ?? f11.toFixed()




function main(): void {}
