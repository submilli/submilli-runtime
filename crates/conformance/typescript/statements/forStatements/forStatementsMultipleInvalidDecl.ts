// @target: es2015
// @allowUnreachableCode: true

interface I {
    id: number;
}

class C implements I {
    id: number;
    valid: boolean;
}

class C2 extends C {
    name: string;
}

/*pruned*/;
              
                  
                    
 

function F(x: string): number { return 42; }

/*pruned*/;  
                    
                     
     

                                                                  
 

// all of these are errors
/*pruned*/;         
for( let a = 1;;){}
for( let a = 'a string';;){}
for( let a = new C();;){}
/*pruned*/;                      
/*pruned*/;        

/*pruned*/;       
for( let b = new C();;){}
for( let b = new C2();;){}

for(let f = F;;){}
for( let f = (x: number) => '';;){}

/*pruned*/;               
for( let arr = [1, 2, 3, 4];;){}
/*pruned*/;                                             

/*pruned*/;                          
/*pruned*/;                                

/*pruned*/;             
/*pruned*/;          

function main(): void {}
