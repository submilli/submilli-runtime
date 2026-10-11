// @target: es2015
// @strictNullChecks: true

const undefinedUnion: { a: number } | undefined = null as unknown as ({ a: number } | undefined);
const nullUnion: { b: number } | null = null as unknown as ({ b: number } | null);

let o1: {} | { a: number } = null as unknown as ({} | { a: number });
let o1_2 = { ...undefinedUnion };

let o2: {} | { b: number } = null as unknown as ({} | { b: number });
let o2_2 = { ...nullUnion };

let o3: {} | { a: number } | { b: number } | { a: number, b: number } = null as unknown as ({} | { a: number } | { b: number } | { a: number, b: number });
let o3_2 = { ...undefinedUnion, ...nullUnion };
let o3_3 = { ...nullUnion, ...undefinedUnion };

let o4: {} | { a: number } = null as unknown as ({} | { a: number });
let o4_2 = { ...undefinedUnion, ...undefinedUnion };

let o5: {} | { b: number } = null as unknown as ({} | { b: number });
let o5_2 = { ...nullUnion, ...nullUnion };



function main(): void {}
