// @target: es2015
// If one operand is the null or undefined value, it is treated as having the type of the other operand.

enum E { a, b, c }

/*pruned*/;                           
let b: number = null as unknown as (number);
/*pruned*/;                       
let d: string = null as unknown as (string);

// null + any
/*pruned*/;            
/*pruned*/;            

// null + number/enum
let r3 = null + b;
let r4 = null + 1;
/*pruned*/;       
let r6 = null + E.a;
let r7 = null + E['a'];
let r8 = b + null;
let r9 = 1 + null;
/*pruned*/;       
let r11 = E.a + null;
let r12 = E['a'] + null;

// null + string
let r13 = null + d;
let r14 = null + '';
let r15 = d + null;
let r16 = '' + null;

function main(): void {}
