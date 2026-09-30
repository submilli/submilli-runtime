// @target: es2015
// If one operand is the null or undefined value, it is treated as having the type of the
// other operand.

let a: boolean = null as unknown as (boolean);
let b: string = null as unknown as (string);
/*pruned*/;                                 

// operator **
let r1a1 = null ** a;
let r1a2 = null ** b;
/*pruned*/;          

let r1b1 = a ** null;
let r1b2 = b ** null;
/*pruned*/;          

let r1c1 = null ** true;
let r1c2 = null ** '';
let r1c3 = null ** {};

let r1d1 = true ** null;
let r1d2 = '' ** null;
let r1d3 = {} ** null;

function main(): void {}
