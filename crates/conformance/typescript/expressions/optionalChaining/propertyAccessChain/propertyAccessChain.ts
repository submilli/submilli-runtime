// @target: es2015
// @strict: true

const o1: undefined | { b: string } = null as unknown as (undefined | { b: string });
o1?.b;

const o2: undefined | { b: { c: string } } = null as unknown as (undefined | { b: { c: string } });
o2?.b.c;

const o3: { b: undefined | { c: string } } = null as unknown as ({ b: undefined | { c: string } });
o3.b?.c;

const o4: { b?: { c: { d?: { e: string } } } } = null as unknown as ({ b?: { c: { d?: { e: string } } } });
o4.b?.c.d?.e;

const o5: { b?(): { c: { d?: { e: string } } } } = null as unknown as ({ b?(): { c: { d?: { e: string } } } });
o5.b?.().c.d?.e;

// GH#33744
/*pruned*/;                                                                                      
/*pruned*/;     

// GH#34109
o1?.b ? 1 : 0;

// GH#36031
o2?.b!.c;
o2?.b!.c!;

function main(): void {}
