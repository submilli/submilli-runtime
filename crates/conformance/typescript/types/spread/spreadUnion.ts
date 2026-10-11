// @target: es2015
let union: { a: number } | { b: string } = null as unknown as ({ a: number } | { b: string });

let o3: { a: number } | { b: string } = null as unknown as ({ a: number } | { b: string });
let o3_2 =  { ...union };

let o4: { a: boolean } | { b: string , a: boolean} = null as unknown as ({ a: boolean } | { b: string , a: boolean});
let o4_2 =  { ...union, a: false };

let o5: { a: number } | { b: string } | { a: number, b: string } = null as unknown as ({ a: number } | { b: string } | { a: number, b: string });
let o5_2 =  { ...union, ...union };

function main(): void {}
