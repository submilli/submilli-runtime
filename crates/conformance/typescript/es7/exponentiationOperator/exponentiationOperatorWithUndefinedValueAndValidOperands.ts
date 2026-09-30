// @target: es2015
// If one operand is the undefined or undefined value, it is treated as having the type of the
// other operand.

enum E {
    a,
    b
}

/*pruned*/;                           
let b: number = null as unknown as (number);

// operator *
/*pruned*/;         
let rk2 = null ** b;
let rk3 = null ** 1;
let rk4 = null ** E.a;
/*pruned*/;         
let rk6 = b ** null;
let rk7 = 0 ** null;
let rk8 = E.b ** null;

function main(): void {}
