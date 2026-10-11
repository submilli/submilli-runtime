// @target: es2015
// @strict: true
// @declaration: true

// Repro from #44438

let a: { a: string } = null as unknown as ({ a: string });
let b: { a?: string } = null as unknown as ({ a?: string });
let c: { a: string | undefined } = null as unknown as ({ a: string | undefined });
let d: { a?: string | undefined } = null as unknown as ({ a?: string | undefined });

let t: boolean = null as unknown as (boolean);

let a1 = { a: 123, ...a };  // string (Error)
let b1 = { a: 123, ...b };  // string | number
let c1 = { a: 123, ...c };  // string | undefined (Error)
let d1 = { a: 123, ...d };  // string | number

let a2 = { a: 123, ...(t ? a : {}) };  // string | number
let b2 = { a: 123, ...(t ? b : {}) };  // string | number
let c2 = { a: 123, ...(t ? c : {}) };  // string | number
let d2 = { a: 123, ...(t ? d : {}) };  // string | number


function main(): void {}
