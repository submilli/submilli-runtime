// @target: es2015
// Note that type guards affect types of variables and parameters only and 
// have no effect on members of objects such as properties. 

// variables in global
let num: number = null as unknown as (number);
let var1: string | number = null as unknown as (string | number);
// Inside function declaration
function f(param: string | number): void {
    // global vars in function declaration
    num =  typeof var1 === "string" && var1.length; // string

    // variables in function declaration
    let var2: string | number = null as unknown as (string | number);
    num = typeof var2 === "string" && var2.length; // string

    // parameters in function declaration
    num = typeof param === "string" && param.length; // string
}
// local function declaration
function f1(param: string | number): void {
    let var2: string | number = null as unknown as (string | number);
    /*pruned*/;                                 
                                              
                                                                

                                                    
                                                                

                                          
                                                                  

                
                                                                         
                                                                
                                                                    
     
}
// Function expression
function f2(param: string | number): void {
    // variables in function declaration
    let var2: string | number = null as unknown as (string | number);
    // variables in function expressions
    /*pruned*/;                                 
                                              
                                                                

                                                    
                                                                

                                          
                                                                  

                
                                                                         
                                                                
                                                                    
              
}
// Arrow expression
function f3(param: string | number): void {
    // variables in function declaration
    let var2: string | number = null as unknown as (string | number);
    // variables in function expressions
    /*pruned*/;                            
                                              
                                                                

                                                    
                                                                

                                          
                                                                  

                
                                                                         
                                                                
                                                                    
              
}
// Return type of function
// Inside function declaration
let strOrNum: string | number = null as unknown as (string | number);
function f4(): string | number {
    let var2: string | number = strOrNum;
    return var2;
}
strOrNum = typeof f4() === "string" && f4(); // string | number 

function main(): void {}
