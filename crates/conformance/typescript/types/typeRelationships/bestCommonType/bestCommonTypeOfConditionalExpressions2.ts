// @target: es2015
// conditional expressions return the best common type of the branches plus contextual type (using the first candidate if multiple BCTs exist)
// these are errors

class Base { foo: string; }
class Derived extends Base { bar: string; }
class Derived2 extends Base { baz: string; }
/*pruned*/;                                
/*pruned*/;                                         
/*pruned*/;                                            

let r2 = true ? 1 : '';
/*pruned*/;                        

function foo<T, U>(t: T, u: U): T | U {
    return true ? t : u;
}

/*pruned*/;                                                                               
                                                    
 

/*pruned*/;                                                
                        
 

function main(): void {}
