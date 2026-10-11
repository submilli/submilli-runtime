// @target: es2015
// A type guard of the form x instanceof C, where C is of a subtype of the global type 'Function' 
// and C has a property named 'prototype'
//  - when true, narrows the type of x to the type of the 'prototype' property in C provided 
//    it is a subtype of the type of x, or
//  - when false, has no effect on the type of x.

interface C1 {
    (): C1;
    prototype: C1;
    p1: string;
}
interface C2 {
    (): C2;
    prototype: C2;
    p2: number;
}
interface D1 extends C1 {
    prototype: D1;
    p3: number;
}
let str: string = null as unknown as (string);
let num: number = null as unknown as (number);
let strOrNum: string | number = null as unknown as (string | number);

/*pruned*/;                          
/*pruned*/;                          
/*pruned*/;                          
/*pruned*/;                                        
/*pruned*/;                              // C1
/*pruned*/;                              // C2
/*pruned*/;                              // C1
/*pruned*/;                              // D1

/*pruned*/;                                        
/*pruned*/;                              // C2
/*pruned*/;                              // D1
/*pruned*/;                              // D1
/*pruned*/;                                       // C2 | D1

function main(): void {}
