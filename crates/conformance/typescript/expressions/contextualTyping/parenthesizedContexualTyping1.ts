// @target: es2015
// @strict: true

/*pruned*/;                              
/*pruned*/;                                              
function fun<T>(g: (x: T) => T, x: T): T {
    return g(x);
}

let a = fun(x => x, 10);
let b = fun((x => x), 10);
let c = fun(((x => x)), 10);
let d = fun((((x => x))), 10);

/*pruned*/;                     
/*pruned*/;                         
/*pruned*/;                             
/*pruned*/;                               

// Ternaries in parens
/*pruned*/;                                                      
/*pruned*/;                                                          
/*pruned*/;                                                                  
/*pruned*/;                                                                            

let lambda1: (x: number) => number = x => x;
let lambda2: (x: number) => number = (x => x);

type ObjType = { x: (p: number) => string; y: (p: string) => number };
/*pruned*/;                                                            
/*pruned*/;                                                              

function main(): void {}
