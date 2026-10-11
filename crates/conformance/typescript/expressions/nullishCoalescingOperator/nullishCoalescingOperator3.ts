// @target: es2015
// @strict: true

const a1: 'literal' | undefined | null = null as unknown as ('literal' | undefined | null);
const a2: '' | undefined | null = null as unknown as ('' | undefined | null);
const a3: 1 | undefined | null = null as unknown as (1 | undefined | null);
const a4: 0 | undefined | null = null as unknown as (0 | undefined | null);
const a5: true | undefined | null = null as unknown as (true | undefined | null);
const a6: false | undefined | null = null as unknown as (false | undefined | null);


const aa1 = a1 ?? a2 ?? a3 ?? a4 ?? a5 ?? a6 ?? 'whatever'


function main(): void {}
