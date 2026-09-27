// @target: es2015
interface I {
    id: number;
}

class C implements I {
    id: number;
}

/*pruned*/;
              
                  
                    
 

function F(x: string): number { return 42; }
function F2(x: number): boolean { return x < 42; }

/*pruned*/;  
                    
                     
     

                                                                  
 

/*pruned*/;  
                    
                   
     

                                                                  
 

let aNumber: number = 'this is a string';
let aString: string = 9.9;
/*pruned*/;           

let aVoid: void = 9.9;

/*pruned*/;                  
/*pruned*/;             
/*pruned*/;                            
let anObjectLiteral: I = { id: 'a string' };
let anOtherObjectLiteral: { id: string } = new C();

let aFunction: typeof F = F2;
let anOtherFunction: (x: string) => number = F2;
let aLambda: typeof F = (x) => 'a string';

/*pruned*/;               
/*pruned*/;                         
/*pruned*/;                             



function main(): void {}
