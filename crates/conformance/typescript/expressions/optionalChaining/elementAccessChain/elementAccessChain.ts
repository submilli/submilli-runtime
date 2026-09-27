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

const o4: { b?: { c: { d?: { e: string } } } } = null as unknown as ({ b?: { c: { d?: { e: string } } } });
o4.b?.["c"].d?.e;
o4.b?.["c"].d?.["e"];

const o5: { b?(): { c: { d?: { e: string } } } } = null as unknown as ({ b?(): { c: { d?: { e: string } } } });
o5.b?.()["c"].d?.e;
o5.b?.()["c"].d?.["e"];
o5["b"]?.()["c"].d?.e;
o5["b"]?.()["c"].d?.["e"];

// GH#33744
/*pruned*/;                                                                                      
/*pruned*/;         

// GH#36031
o2?.["b"]!.c;
o2?.["b"]!["c"];
o2?.["b"]!.c!;
o2?.["b"]!["c"]!;

function main(): void {}
