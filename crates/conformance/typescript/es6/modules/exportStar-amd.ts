// @module: amd
// @target: ES5, ES2015

// @filename: t1.ts
export let x = 1;
export let y = 2;

// @filename: t2.ts
/*pruned*/;            
export function foo(): void { }

// @filename: t3.ts
let x = "x";
let y = "y";
let z = "z";
export { x, y, z };

// @filename: t4.ts
/*pruned*/;          
/*pruned*/;          
/*pruned*/;          

// @filename: main.ts
/*pruned*/;                                
/**/; 
; 
; 
; 
;   
