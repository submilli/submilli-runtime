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
if (typeof strOrBool === "boolean") {
    bool = strOrBool; // boolean
}
else {
    str = strOrBool; // string
}
if (typeof numOrBool === "boolean") {
    bool = numOrBool; // boolean
}
else {
    num = numOrBool; // number
}
if (typeof strOrNumOrBool === "boolean") {
    bool = strOrNumOrBool; // boolean
}
else {
    strOrNum = strOrNumOrBool; // string | number
}
/*pruned*/;                        
                              
 
      
                     
 

if (typeof strOrNum === "boolean") {
    let z1: {} = strOrNum; // {}
}
else {
    let z2: string | number = strOrNum; // string | number
}


// A type guard of the form typeof x !== s, where s is a string literal,
//  - when true, narrows the type of x by typeof x === s when false, or
//  - when false, narrows the type of x by typeof x === s when true.
if (typeof strOrBool !== "boolean") {
    str = strOrBool; // string
}
else {
    bool = strOrBool; // boolean
}
if (typeof numOrBool !== "boolean") {
    num = numOrBool; // number
}
else {
    bool = numOrBool; // boolean
}
if (typeof strOrNumOrBool !== "boolean") {
    strOrNum = strOrNumOrBool; // string | number
}
else {
    bool = strOrNumOrBool; // boolean
}
/*pruned*/;                        
                     
 
      
                              
 

if (typeof strOrNum !== "boolean") {
    let z1: string | number = strOrNum; // string | number
}
else {
    let z2: {} = strOrNum; // {}
}


function main(): void {}
