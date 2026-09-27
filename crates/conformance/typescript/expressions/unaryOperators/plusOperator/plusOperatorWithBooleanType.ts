// @target: es2015
// + operator on boolean type
let BOOLEAN: boolean = null as unknown as (boolean);

function foo(): boolean { return true; }

/**/;    
                       
                                           
 
/*pruned*/;  
                                  
 

/*pruned*/;        

// boolean type var
let ResultIsNumber1 = +BOOLEAN;

// boolean type literal
let ResultIsNumber2 = +true;
let ResultIsNumber3 = +{ x: true, y: false };

// boolean type expressions
/*pruned*/;                   
/*pruned*/;                
let ResultIsNumber6 = +foo();
/*pruned*/;                    

// miss assignment operators
+true;
+BOOLEAN;
+foo();
/*pruned*/;  
/**/;   
/**/;

function main(): void {}
