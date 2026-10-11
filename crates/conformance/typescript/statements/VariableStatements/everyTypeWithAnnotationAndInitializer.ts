// @target: es2015
interface I {
    id: number;
}

class C implements I {
    id: number;
}

/*pruned*/;
              
                  
                    
 

function F(x: string): number { return 42; }

/*pruned*/;  
                    
                     
     

                                                                  
 

let aNumber: number = 9.9;
let aString: string = 'this is a string';
/*pruned*/;                    
/*pruned*/;                         

/*pruned*/;           
/*pruned*/;                     
let aVoid: void = undefined;

let anInterface: I = new C();
let aClass: C = new C();
/*pruned*/;                                    
let anObjectLiteral: I = { id: 12 };
let anOtherObjectLiteral: { id: number } = new C();

let aFunction: typeof F = F;
let anOtherFunction: (x: string) => number = F;
let aLambda: typeof F = (x) => 2;

/*pruned*/;               
/*pruned*/;                         
/*pruned*/;                                                    



function main(): void {}
