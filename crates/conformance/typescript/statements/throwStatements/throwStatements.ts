// @target: es2015
// @strict: true
// @allowUnreachableCode: true

// all legal

interface I {
    id: number;
}

class C implements I {
    id: number;
}

/*pruned*/;
              
                  
                    
 

function F(x: string): number { return 42; }

/*pruned*/;  
                    
                     
     

                                                                  
 

let aNumber = 9.9;
throw aNumber;
let aString = 'this is a string';
throw aString;
/*pruned*/;              
/*pruned*/; 
/*pruned*/;                 
/*pruned*/;    

let anAny = null;
throw anAny;
/*pruned*/;                    
/*pruned*/;      
let anUndefined = undefined;
throw anUndefined;

let aClass = new C();
throw aClass;
/*pruned*/;                         
/*pruned*/;         
let anObjectLiteral = { id: 12 };
throw anObjectLiteral;

let aFunction = F;
throw aFunction;
throw aFunction('');
/*pruned*/;            
/*pruned*/;   
/*pruned*/;      

/*pruned*/;     
/*pruned*/;   
/*pruned*/;    
/*pruned*/;                    
/*pruned*/;          
/*pruned*/;                  
/*pruned*/;             

// no initializer or annotation, so this is an 'any'
/**/; 
/**/;   

// literals
throw 0.0;
throw false;
throw null;
throw undefined;
throw 'a string';
throw function () { return 'a string' };
/*pruned*/;          
throw { x: 12, y: 13 };
throw [];
throw ['a', ['b']];
throw /[a-z]/;
/*pruned*/;      
throw new C();
/*pruned*/;        
/*pruned*/;           


function main(): void {}
