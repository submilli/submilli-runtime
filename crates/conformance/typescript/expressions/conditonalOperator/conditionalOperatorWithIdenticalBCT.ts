// @target: es2015
//Cond ? Expr1 : Expr2,  Expr1 and Expr2 have identical best common type
/*pruned*/;                                                       ;
/*pruned*/;                            ;
/*pruned*/;                            ;

/*pruned*/;                       
/*pruned*/;                       
/*pruned*/;                       

//Cond ? Expr1 : Expr2,  Expr1 is supertype
//Be Not contextually typed
/*pruned*/;  
/*pruned*/;                

//Expr1 and Expr2 are literals
true ? {} : 1;
true ? { a: 1 } : { a: 2, b: 'string' };
let result2 = true ? {} : 1;
let result3 = true ? { a: 1 } : { a: 2, b: 'string' };

//Contextually typed
/*pruned*/;                      
/*pruned*/;                                                                 

//Cond ? Expr1 : Expr2,  Expr2 is supertype
//Be Not contextually typed
/*pruned*/;  
/*pruned*/;                

//Expr1 and Expr2 are literals
true ? 1 : {};
true ? { a: 2, b: 'string' } : { a: 1 };
let result6 = true ? 1 : {};
let result7 = true ? { a: 2, b: 'string' } : { a: 1 };

//Contextually typed
/*pruned*/;                      
/*pruned*/;                                                                 

//Result = Cond ? Expr1 : Expr2,  Result is supertype
//Contextually typed
/*pruned*/;                      
/*pruned*/;                                                                    

//Expr1 and Expr2 are literals
/*pruned*/;                             


function main(): void {}
