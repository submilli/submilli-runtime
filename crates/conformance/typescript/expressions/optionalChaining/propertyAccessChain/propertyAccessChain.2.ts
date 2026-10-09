// @target: es2015
// @strict: true

const o1: undefined | { b: string } = null as unknown as (undefined | { b: string });
o1?.b;

const o2: undefined | { b: { c: string } } = null as unknown as (undefined | { b: { c: string } });
o2?.b.c;

const o3: { b: undefined | { c: string } } = null as unknown as ({ b: undefined | { c: string } });
o3.b?.c;


function main(): void {}
