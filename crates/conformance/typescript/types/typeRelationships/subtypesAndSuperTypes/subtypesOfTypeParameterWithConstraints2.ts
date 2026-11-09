// @target: es2015
// checking whether other types are subtypes of type parameters with constraints

function f1<T extends U, U>(x: T, y: U): void {
    let r = true ? x : y;
    let r_2 = true ? y : x;
}

// V > U > T
function f2<T extends U, U extends V, V>(x: T, y: U, z: V): void {
    let r = true ? x : y;
    let r_2 = true ? y : x;

    // ok
    let r2 = true ? z : y;
    let r2_2 = true ? y : z;

    // ok
    let r2a = true ? z : x;
    let r2b = true ? x : z;
}

// Date > U > T
/*pruned*/;                                                 
                         
                           

         
                                    
                                     

         
                                    
                                     
 


interface I1 { foo: number; }
class C1 { foo: number; }
class C2<T> { foo: T; }
enum E { A }
function f(): void { }
/*pruned*/;  
                       
 
class c { baz: string }
/*pruned*/;  
                       
 

function f4<T extends Number>(x: T): void {
    let r0 = true ? x : null; // ok
    let r0_2 = true ? null : x; // ok

    let u: typeof undefined = null as unknown as (typeof undefined);
    let r0b = true ? u : x; // ok
    let r0b_2 = true ? x : u; // ok
}

function f5<T extends Number>(x: T): void {
    let r1 = true ? 1 : x; // ok
    let r1_2 = true ? x : 1; // ok
}

function f6<T extends String>(x: T): void {
    let r2 = true ? '' : x; // ok
    let r2_2 = true ? x : ''; // ok
}

function f7<T extends Boolean>(x: T): void {
    let r3 = true ? true : x; // ok
    let r3_2 = true ? x : true; // ok
}

/*pruned*/;                              
                                         
                                           
 

function f9<T extends RegExp>(x: T): void {
    let r5 = true ? /1/ : x; // ok
    let r5_2 = true ? x : /1/; // ok
}

function f10<T extends { foo: number }>(x: T): void {
    let r6 = true ? { foo: 1 } : x; // ok
    let r6_2 = true ? x : { foo: 1 }; // ok
}

function f11<T extends () => void>(x: T): void {
    let r7 = true ? () => { } : x; // ok
    let r7_2 = true ? x : () => { }; // ok
}

/*pruned*/;                                         
                                                        
                                                                                                            
 

function f13<T extends I1>(x: T): void {
    let i1: I1 = null as unknown as (I1);
    let r9 = true ? i1 : x; // ok
    let r9_2 = true ? x : i1; // ok
}

function f14<T extends C1>(x: T): void {
    /*pruned*/;                          
    /*pruned*/;              // ok
    /*pruned*/;                // ok
}

function f15<T extends C2<number>>(x: T): void {
    /*pruned*/;                                          
    /*pruned*/;              // ok
    /*pruned*/;                // ok
}

function f16<T extends E>(x: T): void {
    /*pruned*/;             // ok
    /*pruned*/;               // ok

    let r14 = true ? E.A : x; // ok
    let r14_2 = true ? x : E.A; // ok
}

function f17<T extends typeof f>(x: T): void {
    let af: typeof f = null as unknown as (typeof f);
    let r15 = true ? af : x; // ok
    let r15_2 = true ? x : af; // ok
}

/*pruned*/;                                   
                                                     
                                  
                                    
 

function f19<T>(x: T): void {
    function f17<U extends T>(a: U): void {
        let r17 = true ? x : a; // ok
        let r17_2 = true ? a : x; // ok
    }

    function f18<V extends U, U extends T>(a: V): void {
        let r18 = true ? x : a; // ok
        let r18_2 = true ? a : x; // ok
    }
}

function f20<T extends Number>(x: T): void {
    /*pruned*/;                        // ok
    /*pruned*/;                          // ok
}

function f21<T extends Number>(x: T): void {
    let r20 = true ? {} : x; // ok
    let r20_2 = true ? x : {}; // ok
}

function main(): void {}
