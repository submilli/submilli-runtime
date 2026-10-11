// @target: es2015
// @strict: true

// In an intersection everything absorbs unknown

/*pruned*/;                 // null
/*pruned*/;                 // undefined
/*pruned*/;                        // never
/*pruned*/;                   // string
/*pruned*/;                     // string[]
/*pruned*/;                    // unknown
/*pruned*/;                // any

// In a union an unknown absorbs everything

type T10 = unknown | null;  // unknown
type T11 = unknown | undefined;  // unknown
type T12 = unknown | null | undefined;  // unknown
type T13 = unknown | string;  // unknown
type T14 = unknown | string[];  // unknown
type T15 = unknown | unknown;  // unknown
/*pruned*/;                // any

// Type variable and unknown in union and intersection

/*pruned*/;            // T & {}
type T21<T> = T | {};  // T | {}
/*pruned*/;                 // T
type T23<T> = T | unknown;  // unknown

// unknown in conditional types

/*pruned*/;                                      // Deferred
/*pruned*/;                                      // Deferred (so it distributes)
/*pruned*/;                                    // true
/*pruned*/;                                    // Deferred

/*pruned*/;                                        
/*pruned*/;                       // { x: string } | { x: number }
/*pruned*/;           // { x: any }
/*pruned*/;               // { x: unknown }

// keyof unknown

/*pruned*/;            // string | number | symbol
type T41 = keyof unknown;  // never

// Only equality operators are allowed with unknown

function f10(x: unknown): void {
    x == 5;
    x !== 10;
    x >= 0;  // Error
    x.foo;  // Error
    x[10];  // Error
    x();  // Error
    x + 1;  // Error
    x * 2;  // Error
    -x;  // Error
    +x;  // Error
}

// No property accesses, element accesses, or function calls

function f11(x: unknown): void {
    x.foo;  // Error
    x[5];  // Error
    x();  // Error
    new x();  // Error
}

// typeof, instanceof, and user defined type predicates

/*pruned*/;                                                                            

function f20(x: unknown): void {
    if (typeof x === "string" || typeof x === "number") {
        x;  // string | number
    }
    if (x instanceof Error) {
        x;  // Error
    }
    /*pruned*/;         
                       
     
}

// Homomorphic mapped type over unknown

/*pruned*/;                              
/*pruned*/;           // { [x: string]: number }
/*pruned*/;               // {}

// Anything is assignable to unknown

/*pruned*/;                                             
                                                  
            
                
                  
                    
          
             
               
           
 

// unknown assignable only to itself and any

function f22(x: unknown): void {
    /*pruned*/;     
    let v2: unknown = x;
    /*pruned*/;          // Error
    let v4: string = x;  // Error
    let v5: string[] = x;  // Error
    let v6: {} = x;  // Error
    let v7: {} | null | undefined = x;  // Error
}

// Type parameter 'T extends unknown' not related to object

/*pruned*/;                                  
                                
 

// Anything fresh but primitive assignable to { [x: string]: unknown }

/*pruned*/;                                      
           
                 
                           
                      
 

// Locals of type unknown always considered initialized

function f25(): void {
    let x: unknown = null as unknown as (unknown);
    let y = x;
}

// Spread of unknown causes result to be unknown

/*pruned*/;                                    
                                               
                                               
                                                 
                                    
 

// Functions with unknown return type don't need return expressions

function f27(): unknown {
}

// Rest type cannot be created from unknown

function f28(x: unknown): void {
    let { ...a } = x;  // Error
}

// Class properties of type unknown don't need definite assignment

/**/;     
                        
               
           
 

// Type parameter with explicit 'unknown' constraint not assignable to '{}'

/*pruned*/;                                           
                  
                  
 

// Repro from #26796

/*pruned*/;                                          // false
/*pruned*/;                                                                   
/*pruned*/;                                 // false

/*pruned*/;                                   
                         
 


function main(): void {}
