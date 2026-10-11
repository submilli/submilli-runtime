// @target: es2015
// @strict: true
// null is a subtype of any other types except undefined

let r0 = true ? null : null;
let r0_2 = true ? null : null;

let u: typeof undefined = null as unknown as (typeof undefined);
let r0b = true ? u : null;
let r0b_2 = true ? null : u;

let r1 = true ? 1 : null;
let r1_2 = true ? null : 1;

let r2 = true ? '' : null;
let r2_2 = true ? null : '';

let r3 = true ? true : null;
let r3_2 = true ? null : true;

/*pruned*/;                       
/*pruned*/;                         

let r5 = true ? /1/ : null;
let r5_2 = true ? null : /1/;

let r6 = true ? { foo: 1 } : null;
let r6_2 = true ? null : { foo: 1 };

let r7 = true ? () => { } : null;
let r7_2 = true ? null : () => { };

/*pruned*/;                                      
/*pruned*/;                                        // type parameters not identical across declarations

interface I1 { foo: number; }
let i1: I1 = null as unknown as (I1);
let r9 = true ? i1 : null;
let r9_2 = true ? null : i1;

class C1 { foo: number; }
/*pruned*/;                          
/*pruned*/;                
/*pruned*/;                  

class C2<T> { foo: T; }
/*pruned*/;                                          
/*pruned*/;                
/*pruned*/;                  

enum E { A }
/*pruned*/;               
/*pruned*/;                 

let r14 = true ? E.A : null;
let r14_2 = true ? null : E.A;

function f(): void { }
/*pruned*/;  
                       
 
let af: typeof f = null as unknown as (typeof f);
let r15 = true ? af : null;
let r15_2 = true ? null : af;

class c { baz: string }
/*pruned*/;  
                       
 
let ac: typeof c = null as unknown as (typeof c);
let r16 = true ? ac : null;
let r16_2 = true ? null : ac;

function f17<T>(x: T): void {
    let r17 = true ? x : null;
    let r17_2 = true ? null : x;
}

function f18<T, U>(x: U): void {
    let r18 = true ? x : null;
    let r18_2 = true ? null : x;
}
//function f18<T, U extends T>(x: U) {
//    var r18 = true ? x : null;
//    var r18 = true ? null : x;
//}

/*pruned*/;                          
/*pruned*/;                            

let r20 = true ? {} : null;
let r20_2 = true ? null : {};


function main(): void {}
