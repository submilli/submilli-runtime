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
let rk2 = undefined ** b;
let rk3 = undefined ** 1;
let rk4 = undefined ** E.a;
/*pruned*/;         
let rk6 = b ** undefined;
let rk7 = 0 ** undefined;
let rk8 = E.b ** undefined;

function main(): void {}
