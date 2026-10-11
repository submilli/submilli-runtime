//@target: ES5, ES2015

//typeof null is Null
//typeof true is Boolean
//typeof false is Boolean
//typeof numeric literal is Number
//typeof string literal is String
//typeof regex literal is Regex

let nu = null / null;
let u = undefined / undefined;

let b: boolean = null as unknown as (boolean);
let b_2 = true;
let b_3 = false;

let n: number = null as unknown as (number);
let n_2 = 1;
let n_3 = 1.0;
let n_4 = 1e4;
let n_5 = 001; // Error in ES5
let n_6 = 0x1;
let n_7 = -1;
let n_8 = -1.0;
let n_9 = -1e-4;
let n_10 = -003; // Error in ES5
let n_11 = -0x1;

let s: string = null as unknown as (string);
let s_2 = '';
let s_3 = "";
/*pruned*/;    
         
/*pruned*/;    
         

/*pruned*/;                                 
let r_2 = /what/;
let r_3 = /\\\\/;


function main(): void {}
