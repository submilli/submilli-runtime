// @target: es2015
//Cond ? Expr1 : Expr2,  Cond is of object type, Expr1 and Expr2 have the same type
/*pruned*/;                                          

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

function foo(): void { };
class C { static doIt: () => void };

//Cond is an object type variable
/*pruned*/;                      
/*pruned*/;                              
/*pruned*/;                            
/*pruned*/;                            
/*pruned*/;                                
/*pruned*/;                              // union

//Cond is an object type literal
/*pruned*/;                                     
((a: string) => a.length) ? exprBoolean1 : exprBoolean2;
({}) ? exprNumber1 : exprNumber2;
({ a: 1, b: "s" }) ? exprString1 : exprString2;
/*pruned*/;                                        
({ a: 1, b: "s" }) ? exprString1: exprBoolean1; // union

//Cond is an object type expression
/*pruned*/;                 
/*pruned*/;                              
new C() ? exprNumber1 : exprNumber2;
C.doIt() ? exprString1 : exprString2;
/*pruned*/;                                          
/*pruned*/;                              // union

//Results shoud be same as Expr1 and Expr2
/*pruned*/;                                         
/*pruned*/;                                                     
/*pruned*/;                                                  
/*pruned*/;                                                  
/*pruned*/;                                                      
/*pruned*/;                                                             // union

/*pruned*/;                                                        
let resultIsBoolean2 = ((a: string) => a.length) ? exprBoolean1 : exprBoolean2;
let resultIsNumber2 = ({}) ? exprNumber1 : exprNumber2;
let resultIsString2 = ({ a: 1, b: "s" }) ? exprString1 : exprString2;
/*pruned*/;                                                              
let resultIsStringOrBoolean2 = ({ a: 1, b: "s" }) ? exprString1 : exprBoolean1; // union

/*pruned*/;                                    
/*pruned*/;                                                     
let resultIsNumber3 = new C() ? exprNumber1 : exprNumber2;
let resultIsString3 = C.doIt() ? exprString1 : exprString2;
/*pruned*/;                                                                
let resultIsStringOrBoolean3 = C.doIt() ? exprString1 : exprBoolean1; // union


function main(): void {}
