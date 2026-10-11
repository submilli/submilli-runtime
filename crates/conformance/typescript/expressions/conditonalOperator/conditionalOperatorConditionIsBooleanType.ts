// @target: es2015
//Cond ? Expr1 : Expr2,  Cond is of boolean type, Expr1 and Expr2 have the same type
let condBoolean: boolean = null as unknown as (boolean);

/*pruned*/;                                  
let exprBoolean1: boolean = null as unknown as (boolean);
let exprNumber1: number = null as unknown as (number);
let exprString1: string = null as unknown as (string);
/*pruned*/;                                             

/*pruned*/;                                  
let exprBoolean2: boolean = null as unknown as (boolean);
let exprNumber2: number = null as unknown as (number);
let exprString2: string = null as unknown as (string);
/*pruned*/;                                             

//Cond is a boolean type variable
/*pruned*/;                       
condBoolean ? exprBoolean1 : exprBoolean2;
condBoolean ? exprNumber1 : exprNumber2;
condBoolean ? exprString1 : exprString2;
/*pruned*/;                                 
condBoolean ? exprString1 : exprBoolean1; // union

//Cond is a boolean type literal
/*pruned*/;                
false ? exprBoolean1 : exprBoolean2;
true ? exprNumber1 : exprNumber2;
false ? exprString1 : exprString2;
/*pruned*/;                          
true ? exprString1 : exprBoolean1; // union

//Cond is a boolean type expression
/*pruned*/;                 
typeof "123" == "string" ? exprBoolean1 : exprBoolean2;
2 > 1 ? exprNumber1 : exprNumber2;
null === undefined ? exprString1 : exprString2;
/*pruned*/;                                   
null === undefined ? exprString1 : exprBoolean1; // union

//Results shoud be same as Expr1 and Expr2
/*pruned*/;                                          
let resultIsBoolean1 = condBoolean ? exprBoolean1 : exprBoolean2;
let resultIsNumber1 = condBoolean ? exprNumber1 : exprNumber2;
let resultIsString1 = condBoolean ? exprString1 : exprString2;
/*pruned*/;                                                       
let resultIsStringOrBoolean1 = condBoolean ? exprString1 : exprBoolean1; // union

/*pruned*/;                                   
let resultIsBoolean2 = false ? exprBoolean1 : exprBoolean2;
let resultIsNumber2 = true ? exprNumber1 : exprNumber2;
let resultIsString2 = false ? exprString1 : exprString2;
/*pruned*/;                                                
let resultIsStringOrBoolean2 = true ? exprString1 : exprBoolean1; // union
let resultIsStringOrBoolean3 = false ? exprString1 : exprBoolean1; // union

/*pruned*/;                                    
let resultIsBoolean3 = typeof "123" == "string" ? exprBoolean1 : exprBoolean2;
let resultIsNumber3 = 2 > 1 ? exprNumber1 : exprNumber2;
let resultIsString3 = null === undefined ? exprString1 : exprString2;
/*pruned*/;                                                         
let resultIsStringOrBoolean4 = typeof "123" === "string" ? exprString1 : exprBoolean1; // union


function main(): void {}
