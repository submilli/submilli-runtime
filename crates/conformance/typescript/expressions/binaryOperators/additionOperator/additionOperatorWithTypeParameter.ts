// @target: es2015
// type parameter type is not a valid operand of addition operator
enum E { a, b }

function foo<T, U>(t: T, u: U): void {
    /*pruned*/;                            
    /*pruned*/;                                    
    /*pruned*/;                                  
    /*pruned*/;                                  
    /*pruned*/;                                  
    /*pruned*/;                        
    /*pruned*/;                              

    // type parameter as left operand
    /*pruned*/;          // ok, one operand is any
    /*pruned*/;    
    /*pruned*/;    
    /*pruned*/;     // ok, one operand is string
    /*pruned*/;    
    /*pruned*/;    
    /*pruned*/;    

    // type parameter as right operand
    /*pruned*/;     // ok, one operand is any
    /*pruned*/;    
    /*pruned*/;     
    /*pruned*/;      // ok, one operand is string
    /*pruned*/;     
    /*pruned*/;     
    /*pruned*/;     

    // other cases
    let r15 = t + null;
    let r16 = t + undefined;
    let r17 = t + t;
    let r18 = t + u;
    let r19 = t + (() => { });
    let r20 = t + [];
}

function main(): void {}
