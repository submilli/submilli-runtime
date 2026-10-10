// @target: es2015
// @strict: true

// Repro from #31762

/*pruned*/;                          
                                                 
                                                    
 

/*pruned*/;                          
                                                 
                                                       
 

// Object in property or element access is widened when target of assignment

function foo(options?: { a: string, b: number }): void {
    let x1 = (options || {}).a;     // Object type not widened
    let x2 = (options || {})["a"];  // Object type not widened
    (options || {}).a = 1;          // Object type widened, error
    (options || {})["a"] = 1;       // Object type widened, error
}


function main(): void {}
