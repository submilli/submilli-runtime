// @target: es2015
// @strict: true
let f1 = ()
    => { }
let f2 = (x: string, y: string) /*
  */  => { }
/*pruned*/;                             
          
/*pruned*/;                                
            
/*pruned*/;       
          
/*pruned*/;          
            
let f7 = (x: string, y: number, z: number = 10)
    => { }
let f8 = (x: string, y: number, z: number = 10) /*
  */  => { }
let f9 = (a: number): number
    => a;
let f10 = (a: number) :
  number
    => a
let f11 = (a: number): number /*
    */ => a;
let f12 = (a: number) :
  number /*
    */ => a

// Should be valid.
let f11_2 = (a: number
    ) => a;

// Should be valid.
let f12_2 = (a: number)
    : number => a;

// Should be valid.
let f13 = (a: number):
    number => a;

// Should be valid.
let f14 = () /* */ => {}

// Should be valid.
let f15 = (a: number): number /* */ => a

// Should be valid.
let f16 = (a: number, b: number = 10):
  number /* */ => a + b;

function foo(func: () => boolean): void { }
foo(()
    => true);
foo(()
    => { return false; });

/*pruned*/;  
                
                                                       
                     
         

                                   
                        
     

                      
                  
                    
     

                    
                               
 


function main(): void {}
