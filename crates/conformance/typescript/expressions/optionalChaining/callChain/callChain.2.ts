// @target: es2015
// @strict: true

const o1: undefined | (() => number) = null as unknown as (undefined | (() => number));
o1?.();

const o2: undefined | { b: () => number } = null as unknown as (undefined | { b: () => number });
o2?.b();

const o3: { b: (() => { c: string }) | undefined } = null as unknown as ({ b: (() => { c: string }) | undefined });
o3.b?.().c;


function main(): void {}
