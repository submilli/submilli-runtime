// @target: es2015
// ! operator on number type
let NUMBER: number = null as unknown as (number);
let NUMBER1: number[] = [1, 2];

function foo(): number { return 1; }

/**/;    
                      
                                      
 
/*pruned*/;  
                                                
 

/*pruned*/;        

// number type var
let ResultIsBoolean1 = !NUMBER;
let ResultIsBoolean2 = !NUMBER1;

// number type literal
let ResultIsBoolean3 = !1;
let ResultIsBoolean4 = !{ x: 1, y: 2};
let ResultIsBoolean5 = !{ x: 1, y: (n: number) => { return n; } };

// number type expressions
/*pruned*/;                    
/*pruned*/;                 
let ResultIsBoolean8 = !NUMBER1[0];
let ResultIsBoolean9 = !foo();
/*pruned*/;                      
let ResultIsBoolean11 = !(NUMBER + NUMBER);

// multiple ! operator
let ResultIsBoolean12 = !!NUMBER;
let ResultIsBoolean13 = !!!(NUMBER + NUMBER);

// miss assignment operators
!1;
!NUMBER;
!NUMBER1;
!foo();
/**/;   
/**/;
/*pruned*/;  

function main(): void {}
