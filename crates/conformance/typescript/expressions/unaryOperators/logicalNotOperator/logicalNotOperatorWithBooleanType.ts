// @target: es2015
// ! operator on boolean type
let BOOLEAN: boolean = null as unknown as (boolean);

function foo(): boolean { return true; }

/**/;    
                       
                                           
 
/*pruned*/;  
                                                  
 

/*pruned*/;        

// boolean type var
let ResultIsBoolean1 = !BOOLEAN;

// boolean type literal
let ResultIsBoolean2 = !true;
let ResultIsBoolean3 = !{ x: true, y: false };

// boolean type expressions
/*pruned*/;                    
/*pruned*/;                 
let ResultIsBoolean6 = !foo();
/*pruned*/;                     

// multiple ! operators
let ResultIsBoolean = !!BOOLEAN;

// miss assignment operators
!true;
!BOOLEAN;
!foo();
/*pruned*/;  
/**/;   
/**/;

function main(): void {}
