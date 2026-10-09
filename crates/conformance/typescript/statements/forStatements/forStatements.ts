// @target: es2015
// @allowUnreachableCode: true

interface I {
    id: number;
}

class C implements I {
    id: number;
}

/*pruned*/;
              
                  
                    
 

function F(x: string): number { return 42; }

/*pruned*/;  
                    
                     
     

                                                                  
 

for(let aNumber: number = 9.9;;){} 
for(let aString: string = 'this is a string';;){}
/*pruned*/;                            
/*pruned*/;                                 

/*pruned*/;                   
/*pruned*/;                             
for(let aVoid: void = undefined;;){}

for(let anInterface: I = new C();;){}
for(let aClass: C = new C();;){}
/*pruned*/;                                            
for(let anObjectLiteral: I = { id: 12 };;){}
for(let anOtherObjectLiteral: { id: number } = new C();;){}

for(let aFunction: typeof F = F;;){}
for(let anOtherFunction: (x: string) => number = F;;){}
for(let aLambda: typeof F = (x) => 2;;){}

/*pruned*/;                       
/*pruned*/;                                 
/*pruned*/;                                                            

function main(): void {}
