// @target: es2015
// @strict: true

const o1: null | (() => number) = null as unknown as (null | (() => number));
o1?.();

const o2: null | { b: () => number } = null as unknown as (null | { b: () => number });
o2?.b();

const o3: { b: (() => { c: string }) | null } = null as unknown as ({ b: (() => { c: string }) | null });
o3.b?.().c;


function main(): void {}
