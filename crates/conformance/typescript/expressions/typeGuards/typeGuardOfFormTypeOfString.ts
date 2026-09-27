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
if (typeof strOrNum === "string") {
    str = strOrNum; // string
}
else {
    num === strOrNum; // number
}
if (typeof strOrBool === "string") {
    str = strOrBool; // string
}
else {
    bool = strOrBool; // boolean
}
if (typeof strOrNumOrBool === "string") {
    str = strOrNumOrBool; // string
}
else {
    numOrBool = strOrNumOrBool; // number | boolean
}
/*pruned*/;                      
                           
 
      
                    
 

if (typeof numOrBool === "string") {
    let x1: {} = numOrBool; // {}
}
else {
    let x2: number | boolean = numOrBool; // number | boolean
}

// A type guard of the form typeof x !== s, where s is a string literal,
//  - when true, narrows the type of x by typeof x === s when false, or
//  - when false, narrows the type of x by typeof x === s when true.
if (typeof strOrNum !== "string") {
    num === strOrNum; // number
}
else {
    str = strOrNum; // string
}
if (typeof strOrBool !== "string") {
    bool = strOrBool; // boolean
}
else {
    str = strOrBool; // string
}
if (typeof strOrNumOrBool !== "string") {
    numOrBool = strOrNumOrBool; // number | boolean
}
else {
    str = strOrNumOrBool; // string
}
/*pruned*/;                      
                    
 
      
                           
 

if (typeof numOrBool !== "string") {
    let x1: number | boolean = numOrBool; // number | boolean
}
else {
    let x2: {} = numOrBool; // {}
}


function main(): void {}
