// @target: es2015
// @strict: true
// array literals are widened upon assignment according to their element type

let a = []; // any[]
/*pruned*/;    

let a_3 = [null, null];
let a_4 = [null, null];

let b = [[], [null, null]]; // any[][]
let b_2 = [[], []];
let b_3 = [[null, null]];

let c = [[[]]]; // any[][][]
let c_2 = [[[null]],[null]]

// no widening when one or more elements are non-widening

let x: null = null;

let d = [x];
/*pruned*/;     
let d_3 = [null, x];


function main(): void {}
