// @module: commonjs
// @target: es2015
// @Filename: /a.ts
/*pruned*/;                  
/*pruned*/;                  
export type C<T> = T;
export const Value = {};

// @Filename: /b.ts
import type * as types from './a';
types;
types.Value;
let v: types.Value = null as unknown as (types.Value);
/*pruned*/;           
/*pruned*/;           
const c: types.C<string> = "";
const d = { types };
