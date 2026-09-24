// @target: es2015
// @strict: true

const a1: 'literal' | null | null = null as unknown as ('literal' | null | null);
const a2: '' | null | null = null as unknown as ('' | null | null);
const a3: 1 | null | null = null as unknown as (1 | null | null);
const a4: 0 | null | null = null as unknown as (0 | null | null);
const a5: true | null | null = null as unknown as (true | null | null);
const a6: false | null | null = null as unknown as (false | null | null);


const aa1 = a1 ?? a2 ?? a3 ?? a4 ?? a5 ?? a6 ?? 'whatever'


function main(): void {}
