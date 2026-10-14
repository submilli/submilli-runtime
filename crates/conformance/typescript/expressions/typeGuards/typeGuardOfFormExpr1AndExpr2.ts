// @target: es2015
// @strict: true
let str: string = null as unknown as (string);
let bool: boolean = null as unknown as (boolean);
let num: number = null as unknown as (number);
let strOrNum: string | number = null as unknown as (string | number);
let strOrNumOrBool: string | number | boolean = null as unknown as (string | number | boolean);
let numOrBool: number | boolean = null as unknown as (number | boolean);
/*pruned*/;           
/*pruned*/;                       
/*pruned*/;                                               
/*pruned*/;                                                                                               

// A type guard of the form expr1 && expr2
//  - when true, narrows the type of x by expr1 when true and then by expr2 when true, or
//  - when false, narrows the type of x to T1 | T2, where T1 is the type of x narrowed by expr1 when 
//    false, and T2 is the type of x narrowed by expr1 when true and then by expr2 when false.

// (typeguard1 && typeguard2)
if (typeof strOrNumOrBool !== "string" && typeof strOrNumOrBool !== "number") {
    bool = strOrNumOrBool; // boolean
}
else {
    strOrNum = strOrNumOrBool; // string | number
}
// (typeguard1 && typeguard2 && typeguard3)
/*pruned*/;                                                                                                                    
                               
 
      
                                                                    
 
// (typeguard1 && typeguard2 && typeguard11(onAnotherType))
/*pruned*/;                                                                                                                 
                                               
                                     
 
      
                                                                                               
                                                       
 
// (typeguard1) && simpleExpr
if (typeof strOrNumOrBool !== "string" && numOrBool !== strOrNumOrBool) {
    numOrBool = strOrNumOrBool; // number | boolean
}
else {
    let r3: string | number | boolean = strOrNumOrBool; // string | number | boolean
}

function main(): void {}
