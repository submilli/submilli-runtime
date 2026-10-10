// @target: es2015
// @strict: true

// From #4260
class X<T> {
    f(t: T): { a: T; } {
        return { a: t };
    }
}

/*pruned*/;                                     
/*pruned*/;                             

                                                                       
/*pruned*/;  // no error because f accepts number
/*pruned*/;                                  

                                                 
/*pruned*/;                                     

                                                             
/*pruned*/;  // error, unknown still expects an argument

const xNever: X<never> = null as unknown as (X<never>);
xNever.f() // error, never still expects an argument


// Promise has previously been updated to work without arguments, but to show this fixes the issue too.

class MyPromise<X> {
    constructor(executor: (resolve: (value: X) => void) => void) {

    }
}

new MyPromise<void>(resolve => resolve()); // no error
new MyPromise<void | number>(resolve => resolve()); // no error
/*pruned*/;                               // error, `any` arguments cannot be omitted
new MyPromise<unknown>(resolve => resolve()); // error, `unknown` arguments cannot be omitted
new MyPromise<never>(resolve => resolve()); // error, `never` arguments cannot be omitted


// Multiple parameters

function a(x: number, y: string, z: void): void  {
    
}

a(4, "hello"); // ok
a(4, "hello", void 0); // ok
a(4); // not ok

function b(x: number, y: string, z: void, what: number): void  {
    
}

b(4, "hello", void 0, 2); // ok
b(4, "hello"); // not ok
b(4, "hello", void 0); // not ok
b(4); // not ok

function c(x: number | void, y: void, z: void | string | number): void  {
    
}

c(3, void 0, void 0); // ok
c(3, void 0); // ok
c(3); // ok
c(); // ok


// Spread Parameters

/*pruned*/;                         
                                      
                          

/*pruned*/;                           // error
/*pruned*/;                                 // ok

/*pruned*/;                                // ok
/*pruned*/;                        // ok
/*pruned*/;                    // ok
/*pruned*/;                                      // ok
/*pruned*/;                                         // ok
/*pruned*/;                                            // ok


function main(): void {}
