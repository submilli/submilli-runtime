// @target: es2015
// A parameter declaration may specify either an identifier or a binding pattern.
// The identifiers specified in parameter declarations and binding patterns
// in a parameter list must be unique within that parameter list.

// If the declaration includes a type annotation, the parameter is of that type
/*pruned*/;                                                       
function a2(o: { x: number, a: number }): void { }
/*pruned*/;                                                                                                                         ;
function a4({x, a}: { x: number, a: number }): void { }

/*pruned*/;             
/*pruned*/;                

// If the declaration includes an initializer expression (which is permitted only
// when the parameter list occurs in conjunction with a function body),
// the parameter type is the widened form (section 3.11) of the type of the initializer expression.

function b1(z: (null | undefined)[] = [undefined, null]): void { };
function b2(z: null = null, o: { x: number; y: undefined; } = { x: 0, y: undefined }): void { }
/*pruned*/;                                                              

/*pruned*/;   
                                        
 

function b6([a, z, y] = [undefined, null, undefined]): void { }
/*pruned*/;                                                                                   

b1([1, 2, 3]);  // z is widen to the type any[]
b2("string", { x: 200, y: "string" });
b2("string", { x: 200, y: true });
b6(["string", 1, 2]);                    // Shouldn't be an error
/*pruned*/;                              // Shouldn't be an error


// If the declaration specifies a binding pattern, the parameter type is the implied type of that binding pattern (section 5.1.3)
enum Foo { a }
/*pruned*/;                            
function c1({z} = { z: 10 }): void { }
/*pruned*/;                    
function c3({b}: { b: number|string} = { b: "hello" }): void { }
/*pruned*/;                         
/*pruned*/;                           

/*pruned*/;                                 // Implied type is { z: {x: any, y: {j: any}} }
/*pruned*/;                                 // Implied type is { z: {x: any, y: {j: any}} }

c1();             // Implied type is {z:number}?
c1({ z: 1 })      // Implied type is {z:number}? 

/**/;           // Implied type is {z?: number}
/**/;           // Implied type is {z?: number}

c3({ b: 1 });     // Implied type is { b: number|string }.

/*pruned*/;                             // Implied type is is [any, any, [[any]]]
/*pruned*/;                             // Implied type is is [any, any, [[any]]]

// A parameter can be marked optional by following its name or binding pattern with a question mark (?)
// or by including an initializer.

/*pruned*/;              
function d0(x: number = 10): void { }

/*pruned*/;   
                   
                   
                  
 

/*pruned*/;             
                     
                  
                  
                           
 

/*pruned*/;             
                           
                           
                           
 


function d5({x, y} = { x: 1, y: 2 }): void { }
d5();  // Parameter is optional as its declaration included an initializer

// Destructuring parameter declarations do not permit type annotations on the individual binding patterns,
// as such annotations would conflict with the already established meaning of colons in object literals.
// Type annotations must instead be written on the top- level parameter declaration

/*pruned*/;                         // x has type any NOT number
function e2({x}: { x: number }): void { }  // x is type number
function e3({x}: { x?: number }): void { }  // x is an optional with type number
/*pruned*/;                                       // x has type [any, any, any]
/*pruned*/;                                                             // x has type [any, any, any]


function main(): void {}
