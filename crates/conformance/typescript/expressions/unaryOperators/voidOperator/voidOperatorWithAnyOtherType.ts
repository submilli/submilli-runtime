// @target: es2015
// @noImplicitAny: false
// void  operator on any type

/*pruned*/;                             
let ANY1;
/*pruned*/;                
let obj: () => {} = null as unknown as (() => {});
let obj1 = {x:"",y:1};

/*pruned*/;          
                                           
             
 
/**/;    
                  
                       
                                               
                 
     
 
/*pruned*/;  
                                           
 
/*pruned*/;        

// any type var
let ResultIsAny1 = void ANY1;
/*pruned*/;                  
/*pruned*/;               
/*pruned*/;               
let ResultIsAny5 = void obj;
let ResultIsAny6 = void obj1;

// any type literal
let ResultIsAny7 = void undefined;
let ResultIsAny8 = void null;

// any type expressions
/*pruned*/;                    
let ResultIsAny10 = void obj1.x;
let ResultIsAny11 = void obj1.y;
/*pruned*/;                     
/*pruned*/;                  
/*pruned*/;                    
/*pruned*/;                      
/*pruned*/;                           
let ResultIsAny17 = void (null + undefined);
let ResultIsAny18 = void (null + null);
let ResultIsAny19 = void (undefined + undefined);

// multiple void  operators
/*pruned*/;                       
/*pruned*/;                                     

// miss assignment operators
/**/;    
void ANY1;
/*pruned*/;  
/*pruned*/;    
/*pruned*/; 
/**/;    

function main(): void {}
