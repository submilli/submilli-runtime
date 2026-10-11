// @target: es2015
// If one operand is the null or undefined value, it is treated as having the type of the other operand.

function foo(): void { return undefined }

let a: boolean = null as unknown as (boolean);
/*pruned*/;                                 
let c: void = null as unknown as (void);
/*pruned*/;                                 

// undefined + boolean/Object
let r1 = undefined + a;
/*pruned*/;            
let r3 = undefined + c;
let r4 = a + undefined;
/*pruned*/;            
let r6 = undefined + c;

// other cases
/*pruned*/;            
let r8 = undefined + true;
let r9 = undefined + { a: '' };
let r10 = undefined + foo();
let r11 = undefined + (() => { });

function main(): void {}
