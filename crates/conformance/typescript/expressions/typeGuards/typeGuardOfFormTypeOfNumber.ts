// @target: es2015
class C { private p: string };

let str: string = null as unknown as (string);
let bool: boolean = null as unknown as (boolean);
let num: number = null as unknown as (number);
let strOrNum: string | number = null as unknown as (string | number);
let strOrBool: string | boolean = null as unknown as (string | boolean);
let numOrBool: number | boolean = null as unknown as (number | boolean);
let strOrNumOrBool: string | number | boolean = null as unknown as (string | number | boolean);
/*pruned*/;                                              
/*pruned*/;                                              
/*pruned*/;                                                 
/*pruned*/;                       

//	A type guard of the form typeof x === s, 
//  where s is a string literal with the value 'string', 'number', or 'boolean',
//  - when true, narrows the type of x to the given primitive type, or
//  - when false, removes the primitive type from the type of x.
if (typeof strOrNum === "number") {
    num = strOrNum; // number
}
else {
    str === strOrNum; // string
}
if (typeof numOrBool === "number") {
    num = numOrBool; // number
}
else {
    let x: number | boolean = numOrBool; // number | boolean
}
if (typeof strOrNumOrBool === "number") {
    num = strOrNumOrBool; // number
}
else {
    strOrBool = strOrNumOrBool; // string | boolean
}
/*pruned*/;                      
                           
 
      
                    
 

if (typeof strOrBool === "number") {
    let y1: {} = strOrBool; // {}
}
else {
    let y2: string | boolean = strOrBool; // string | boolean
}

// A type guard of the form typeof x !== s, where s is a string literal,
//  - when true, narrows the type of x by typeof x === s when false, or
//  - when false, narrows the type of x by typeof x === s when true.
if (typeof strOrNum !== "number") {
    str === strOrNum; // string
}
else {
    num = strOrNum; // number
}
if (typeof numOrBool !== "number") {
    let x: number | boolean = numOrBool; // number | boolean
}
else {
    num = numOrBool; // number
}
if (typeof strOrNumOrBool !== "number") {
    strOrBool = strOrNumOrBool; // string | boolean
}
else {
    num = strOrNumOrBool; // number
}
/*pruned*/;                      
                    
 
      
                           
 

if (typeof strOrBool !== "number") {
    let y1: string | boolean = strOrBool; // string | boolean
}
else {
    let y2: {} = strOrBool; // {}
}


function main(): void {}
