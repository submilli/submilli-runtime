// @target: es2015
// If one operand is the null or undefined value, it is treated as having the type of the other operand.

function foo(): void { return undefined }

let a: boolean = null as unknown as (boolean);
/*pruned*/;                                 
let c: void = null as unknown as (void);
/*pruned*/;                                 

// null + boolean/Object
let r1 = null + a;
/*pruned*/;       
let r3 = null + c;
let r4 = a + null;
/*pruned*/;       
let r6 = null + c;

// other cases
/*pruned*/;       
let r8 = null + true;
let r9 = null + { a: '' };
let r10 = null + foo();
let r11 = null + (() => { });

function main(): void {}
