// @target: es2015
// @strict: true
// When a function expression is inferentially typed (section 4.9.3) and a type assigned to a parameter in that expression references type parameters for which inferences are being made, 
// the corresponding inferred type arguments to become fixed and no further candidate inferences are made for them.

function foo<T>(a: (x: T) => T, b: (x: T) => T): (x: T) => T {
    let r: (x: T) => T = null as unknown as ((x: T) => T);
    return r;
}

//var r1 = foo((x: number) => 1, (x: string) => ''); // error
let r1b = foo((x) => 1, (x) => ''); // {} => {}
/*pruned*/;                                           // Object => Object
/*pruned*/;                                          // number => number
let r3ii = foo((x: number) => 1, (x: number) => 1); // number => number

let a: { x: number; y?: number; } = null as unknown as ({ x: number; y?: number; });
let b: { x: number; z?: number; } = null as unknown as ({ x: number; z?: number; });

let r4 = foo((x: typeof a) => a, (x: typeof b) => b); // typeof a => typeof a
let r5 = foo((x: typeof b) => b, (x: typeof a) => a); // typeof b => typeof b

function other<T>(x: T): void {
    let r6 = foo((a: T) => a, (b: T) => b); // T => T
    let r6b = foo((a) => a, (b) => b); // {} => {}
}

/*pruned*/;                                  
                                                     
                                                  
                      
                 
                              
 


/*pruned*/;                                                                 
                                                          
             
 

/*pruned*/;                                    
                                                                  
 

function main(): void {}
