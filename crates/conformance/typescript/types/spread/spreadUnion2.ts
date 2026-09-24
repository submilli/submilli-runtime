// @target: es2015
// @strictNullChecks: true

const undefinedUnion: { a: number } | null = null as unknown as ({ a: number } | null);
const nullUnion: { b: number } | null = null as unknown as ({ b: number } | null);

let o1: {} | { a: number } = null as unknown as ({} | { a: number });
let o1 = { ...undefinedUnion };

let o2: {} | { b: number } = null as unknown as ({} | { b: number });
let o2 = { ...nullUnion };

let o3: {} | { a: number } | { b: number } | { a: number, b: number } = null as unknown as ({} | { a: number } | { b: number } | { a: number, b: number });
let o3 = { ...undefinedUnion, ...nullUnion };
let o3 = { ...nullUnion, ...undefinedUnion };

let o4: {} | { a: number } = null as unknown as ({} | { a: number });
let o4 = { ...undefinedUnion, ...undefinedUnion };

let o5: {} | { b: number } = null as unknown as ({} | { b: number });
let o5 = { ...nullUnion, ...nullUnion };



function main(): void {}
