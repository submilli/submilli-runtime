// @target: es2015
// If one operand is the undefined or undefined value, it is treated as having the type of the
// other operand.

let a: boolean = null as unknown as (boolean);
let b: string = null as unknown as (string);
/*pruned*/;                                 

// operator **
let r1a1 = undefined ** a;
let r1a2 = undefined ** b;
/*pruned*/;               

let r1b1 = a ** undefined;
let r1b2 = b ** undefined;
/*pruned*/;               

let r1c1 = undefined ** true;
let r1c2 = undefined ** '';
let r1c3 = undefined ** {};

let r1d1 = true ** undefined;
let r1d2 = '' ** undefined;
let r1d3 = {} ** undefined;

function main(): void {}
