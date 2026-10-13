// @module: commonjs
// @target: es2015
// @Filename: /abc.ts
export class A {}
export type B  = { b: string };
export const C = "";

// @Filename: /d.ts
import type { A, B, C } from './abc';
new A();
/*pruned*/;                       
let b: B = null as unknown as (B);
b.b;
const c = { A };
