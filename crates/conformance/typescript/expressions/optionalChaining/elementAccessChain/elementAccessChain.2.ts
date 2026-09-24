// @target: es2015
// @strict: true

const o1: null | { b: string } = null as unknown as (null | { b: string });
o1?.["b"];

const o2: null | { b: { c: string } } = null as unknown as (null | { b: { c: string } });
o2?.["b"].c;
o2?.b["c"];

const o3: { b: null | { c: string } } = null as unknown as ({ b: null | { c: string } });
o3["b"]?.c;
o3.b?.["c"];


function main(): void {}
