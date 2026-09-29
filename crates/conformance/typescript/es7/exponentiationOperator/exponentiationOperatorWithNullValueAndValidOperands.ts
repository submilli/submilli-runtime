// @target: es2015
// If one operand is the null or undefined value, it is treated as having the type of the
// other operand.

enum E {
    a,
    b
}

/*pruned*/;                           
let b: number = null as unknown as (number);

// operator **
/*pruned*/;        
let r2 = null ** b;
let r3 = null ** 1;
let r4 = null ** E.a;
/*pruned*/;        
let r6 = b ** null;
let r7 = 0 ** null;
let r8 = E.b ** null;

function main(): void {}
