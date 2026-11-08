// @target: es2015
// type parameters are not assignable to one another unless directly or indirectly constrained to one another

function foo<T, U extends T>(t: T, u: U): void {
    t = u; // error
    u = t; // ok
}

function foo2<T extends U, U>(t: T, u: U): void {
    t = u; // error
    u = t; // ok
}

function foo3<T extends U, U extends V, V>(t: T, u: U, v: V): void {
    t = u; // error
    u = t;

    t = v; // error
    v = t; // ok

    u = v; // error
    v = u; // ok
}

/*pruned*/;                                                                      
                   
                   
                            

          
                   
                            

          
          
                         

                                            
                
                
                
 

// same as foo4 with different type parameter ordering
/*pruned*/;                                                                      
                   
                   
                            

          
                   
                            

          
          
                         

                                            
                
                
                
 

function foo6<T extends U, U, V>(t: T, u: U, v: V): void {
    t = u; // error
    t = v; // error

    u = t; // ok
    u = v; // error

    v = t; // error
    v = u; // error
}

function main(): void {}
