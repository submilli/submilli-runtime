// @target: es2015
// @strict: true

const a: { p: string | undefined, m(): string | undefined } = null as unknown as ({ p: string | undefined, m(): string | undefined });
const b: { p: string | undefined, m(): string | undefined } = null as unknown as ({ p: string | undefined, m(): string | undefined });

const n1 = a.p ?? "default";
const n2 = a.m() ?? "default";
const n3 = a.m() ?? b.p ?? b.m() ?? "default";;


function main(): void {}
