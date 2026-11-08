// @target: es2015
// it is an error to use a generic type without type arguments
// all of these are errors 

class C<T> {
    foo: T;
}

let c: C = null as unknown as (C);

let a: { x: C } = null as unknown as ({ x: C });
/*pruned*/;                                               
let d: { [x: C]: C } = null as unknown as ({ [x: C]: C });

let e = (x: C) => { let y: C = null as unknown as (C); return y; }

function f(x: C): C { let y: C = null as unknown as (C); return y; }

let g = function f(x: C): C { let y: C = null as unknown as (C); return y; }

class D extends C {
}

interface I extends C {}

/*pruned*/;  
                                
 

/*pruned*/;             
/*pruned*/;                
/*pruned*/;                 

function h<T extends C>(x: T): void { }
/*pruned*/;                              

let j = <C>null;
/*pruned*/;       

function main(): void {}
