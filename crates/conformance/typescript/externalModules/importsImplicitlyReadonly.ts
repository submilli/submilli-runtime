// @target: es2015
// @module: commonjs

// @filename: a.ts
export let x = 1;
let y = 1;
export { y };

// @filename: b.ts
import { x, y } from "./a";
import * as a1 from "./a";
/*pruned*/;                
const a3 = a1;

x = 1;     // Error
y = 1;     // Error
a1.x = 1;  // Error
a1.y = 1;  // Error
/**/;    
/**/;    
a3.x = 1;
a3.y = 1;