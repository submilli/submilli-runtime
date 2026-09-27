// @target: es2015
// + operator on number type
let NUMBER: number = null as unknown as (number);
let NUMBER1: number[] = [1, 2];

function foo(): number { return 1; }

/**/;    
                      
                                      
 
/*pruned*/;  
                             
 

/*pruned*/;        

// number type var
let ResultIsNumber1 = +NUMBER;
let ResultIsNumber2 = +NUMBER1;

// number type literal
let ResultIsNumber3 = +1;
let ResultIsNumber4 = +{ x: 1, y: 2};
let ResultIsNumber5 = +{ x: 1, y: (n: number) => { return n; } };

// number type expressions
/*pruned*/;                   
/*pruned*/;                
let ResultIsNumber8 = +NUMBER1[0];
let ResultIsNumber9 = +foo();
/*pruned*/;                     
let ResultIsNumber11 = +(NUMBER + NUMBER);

// miss assignment operators
+1;
+NUMBER;
+NUMBER1;
+foo();
/**/;   
/**/;
/*pruned*/;  

function main(): void {}
