// @target: es2015
// @strict: true
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
let aString = 'this is a string';
/*pruned*/;              
/*pruned*/;                 

let anAny = null;
/*pruned*/;                    
let anUndefined = undefined;


let aClass = new C();
/*pruned*/;                         
let anObjectLiteral = { id: 12 };

let aFunction = F;
/*pruned*/;            

/*pruned*/;     
/*pruned*/;                    
/*pruned*/;                  

// no initializer or annotation, so this is an 'any'
/**/; 



function main(): void {}
