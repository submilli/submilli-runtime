// @target: es2015
// @strict: true
let indexed1: { [n: string]: number; a: number; } = null as unknown as ({ [n: string]: number; a: number; });
let indexed2: { [n: string]: boolean; c: boolean; } = null as unknown as ({ [n: string]: boolean; c: boolean; });
let indexed3: { [n: string]: number } = null as unknown as ({ [n: string]: number });
let i = { ...indexed1, b: 11 };
// only indexed has indexer, so i[101]: any
/*pruned*/
let ii = { ...indexed1, ...indexed2 };
// both have indexer, so i[1001]: number | boolean
/*pruned*/

const b: boolean = null as unknown as (boolean);
/*pruned*/

let roindex: { readonly [x:string]: number } = null as unknown as ({ readonly [x:string]: number });
let writable = { ...roindex };
writable.a = 0;  // should be ok.


function main(): void {}
