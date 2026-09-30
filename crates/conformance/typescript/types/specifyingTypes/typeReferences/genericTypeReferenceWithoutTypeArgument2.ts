// @target: es2015
// it is an error to use a generic type without type arguments
// all of these are errors 

interface I<T> {
    foo: T;
}

let c: I = null as unknown as (I);

let a: { x: I } = null as unknown as ({ x: I });
/*pruned*/;                                               
/*pruned*/;                                               

let e = (x: I) => { let y: I = null as unknown as (I); return y; }

function f(x: I): I { let y: I = null as unknown as (I); return y; }

let g = function f(x: I): I { let y: I = null as unknown as (I); return y; }

class D extends I {
}

interface U extends I {}

/*pruned*/;  
                                    
 

/*pruned*/;             
/*pruned*/;                    
/*pruned*/;                 

/*pruned*/;                            
/*pruned*/;                              

let j = <C>null;
/*pruned*/;       

function main(): void {}
