// @target: es2015
//Cond ? Expr1 : Expr2,  Cond is of number type, Expr1 and Expr2 have the same type
let condNumber: number = null as unknown as (number);

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

//Cond is a number type variable
/*pruned*/;                      
condNumber ? exprBoolean1 : exprBoolean2;
condNumber ? exprNumber1 : exprNumber2;
condNumber ? exprString1 : exprString2;
/*pruned*/;                                
condNumber ? exprString1 : exprBoolean1; // Union

//Cond is a number type literal
/*pruned*/;             
0 ? exprBoolean1 : exprBoolean2;
0.123456789 ? exprNumber1 : exprNumber2;
- 10000000000000 ? exprString1 : exprString2;
/*pruned*/;                                   
10000 ? exprString1 : exprBoolean1; // Union

//Cond is a number type expression
function foo(): number { return 1 };
let array = [1, 2, 3];

/*pruned*/;                 
1 + 1 ? exprBoolean1 : exprBoolean2;
"string".length ? exprNumber1 : exprNumber2;
foo() ? exprString1 : exprString2;
/*pruned*/;                                      
foo() ? exprString1 : exprBoolean1; // Union

//Results shoud be same as Expr1 and Expr2
/*pruned*/;                                         
let resultIsBoolean1 = condNumber ? exprBoolean1 : exprBoolean2;
let resultIsNumber1 = condNumber ? exprNumber1 : exprNumber2;
let resultIsString1 = condNumber ? exprString1 : exprString2;
/*pruned*/;                                                      
let resultIsStringOrBoolean1 = condNumber ? exprString1 : exprBoolean1; // Union

/*pruned*/;                                
let resultIsBoolean2 = 0 ? exprBoolean1 : exprBoolean2;
let resultIsNumber2 = 0.123456789 ? exprNumber1 : exprNumber2;
let resultIsString2 = - 10000000000000 ? exprString1 : exprString2;
/*pruned*/;                                                         
let resultIsStringOrBoolean2 = 10000 ? exprString1 : exprBoolean1; // Union

/*pruned*/;                                    
let resultIsBoolean3 = 1 + 1 ? exprBoolean1 : exprBoolean2;
let resultIsNumber3 = "string".length ? exprNumber1 : exprNumber2;
let resultIsString3 = foo() ? exprString1 : exprString2;
/*pruned*/;                                                            
let resultIsStringOrBoolean3 = foo() / array[1] ? exprString1 : exprBoolean1; // Union

function main(): void {}
