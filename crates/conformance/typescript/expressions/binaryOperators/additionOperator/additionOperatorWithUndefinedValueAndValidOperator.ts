// @target: es2015
// If one operand is the null or undefined value, it is treated as having the type of the other operand.

enum E { a, b, c }

/*pruned*/;                           
let b: number = null as unknown as (number);
/*pruned*/;                       
let d: string = null as unknown as (string);

// undefined + any
/*pruned*/;                 
/*pruned*/;                 

// undefined + number/enum
let r3 = undefined + b;
let r4 = undefined + 1;
/*pruned*/;            
let r6 = undefined + E.a;
let r7 = undefined + E['a'];
let r8 = b + undefined;
let r9 = 1 + undefined;
/*pruned*/;            
let r11 = E.a + undefined;
let r12 = E['a'] + undefined;

// undefined + string
let r13 = undefined + d;
let r14 = undefined + '';
let r15 = d + undefined;
let r16 = '' + undefined;

function main(): void {}
