// @target: es2015
// @strict: true
// array literals are widened upon assignment according to their element type

let a = []; // any[]
/*pruned*/;    

let a_3 = [null, null];
let a_4 = [undefined, undefined];

let b = [[], [null, null]]; // any[][]
let b_2 = [[], []];
let b_3 = [[undefined, undefined]];

let c = [[[]]]; // any[][][]
let c_2 = [[[null]],[undefined]]

// no widening when one or more elements are non-widening

let x: undefined = undefined;

let d = [x];
/*pruned*/;     
let d_3 = [undefined, x];


function main(): void {}
