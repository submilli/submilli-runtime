// @target: es2015
// @strict: true

const a1: 'literal' | undefined | null = null as unknown as ('literal' | undefined | null);
const a2: '' | undefined | null = null as unknown as ('' | undefined | null);
const a3: 1 | undefined | null = null as unknown as (1 | undefined | null);
const a4: 0 | undefined | null = null as unknown as (0 | undefined | null);
const a5: true | undefined | null = null as unknown as (true | undefined | null);
const a6: false | undefined | null = null as unknown as (false | undefined | null);
const a7: unknown | null = null as unknown as (unknown | null);
const a8: never | null = null as unknown as (never | null);
/*pruned*/;                                            


const aa1 = a1 ?? 'whatever'
const aa2 = a2 ?? 'whatever'
const aa3 = a3 ?? 'whatever'
const aa4 = a4 ?? 'whatever'
const aa5 = a5 ?? 'whatever'
const aa6 = a6 ?? 'whatever'
const aa7 = a7 ?? 'whatever'
const aa8 = a8 ?? 'whatever'
/*pruned*/;                 

function main(): void {}
