// @target: es2015
// @strict: true

const a: { p: string | null, m(): string | null } = null as unknown as ({ p: string | null, m(): string | null });
const b: { p: string | null, m(): string | null } = null as unknown as ({ p: string | null, m(): string | null });

const n1 = a.p ?? "default";
const n2 = a.m() ?? "default";
const n3 = a.m() ?? b.p ?? b.m() ?? "default";;


function main(): void {}
