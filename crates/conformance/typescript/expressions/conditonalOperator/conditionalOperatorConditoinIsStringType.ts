// @target: es2015
//Cond ? Expr1 : Expr2,  Cond is of string type, Expr1 and Expr2 have the same type
let condString: string = null as unknown as (string);

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

//Cond is a string type variable
/*pruned*/;                      
condString ? exprBoolean1 : exprBoolean2;
condString ? exprNumber1 : exprNumber2;
condString ? exprString1 : exprString2;
/*pruned*/;                                
condString ? exprString1 : exprBoolean1; // union

//Cond is a string type literal
/*pruned*/;              
"string" ? exprBoolean1 : exprBoolean2;
'c' ? exprNumber1 : exprNumber2;
'string' ? exprString1 : exprString2;
/*pruned*/;                          
"hello " ? exprString1 : exprBoolean1; // union

//Cond is a string type expression
function foo(): string { return "string" };
let array = ["1", "2", "3"];

/*pruned*/;                             
condString.toUpperCase ? exprBoolean1 : exprBoolean2;
condString + "string" ? exprNumber1 : exprNumber2;
foo() ? exprString1 : exprString2;
/*pruned*/;                              
foo() ? exprString1 : exprBoolean1; // union

//Results shoud be same as Expr1 and Expr2
/*pruned*/;                                         
let resultIsBoolean1 = condString ? exprBoolean1 : exprBoolean2;
let resultIsNumber1 = condString ? exprNumber1 : exprNumber2;
let resultIsString1 = condString ? exprString1 : exprString2;
/*pruned*/;                                                      
let resultIsStringOrBoolean1 = condString ? exprString1 : exprBoolean1; // union

/*pruned*/;                                 
let resultIsBoolean2 = "string" ? exprBoolean1 : exprBoolean2;
let resultIsNumber2 = 'c' ? exprNumber1 : exprNumber2;
let resultIsString2 = 'string' ? exprString1 : exprString2;
/*pruned*/;                                                
let resultIsStringOrBoolean2 = "hello" ? exprString1 : exprBoolean1; // union

/*pruned*/;                                                
let resultIsBoolean3 = condString.toUpperCase ? exprBoolean1 : exprBoolean2;
let resultIsNumber3 = condString + "string" ? exprNumber1 : exprNumber2;
let resultIsString3 = foo() ? exprString1 : exprString2;
/*pruned*/;                                                    
/*pruned*/;                                                                    // union
let resultIsStringOrBoolean4 = condString.toUpperCase ? exprString1 : exprBoolean1; // union

function main(): void {}
