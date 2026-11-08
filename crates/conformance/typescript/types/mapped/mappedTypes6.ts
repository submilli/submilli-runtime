// @target: es2015
// @strict: true
// @declaration: true

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
/*pruned*/;                                        
/*pruned*/;                                        

/*pruned*/;                                      
/*pruned*/;                                       
/*pruned*/;                                        
/*pruned*/;                                        

function f1<T>(x: Required<T>, y: T, z: Partial<T>): void {
    x = x;
    x = y;  // Error
    x = z;  // Error
    y = x;
    y = y;
    y = z;  // Error
    z = x;
    z = y;
    z = z;
}

/*pruned*/;                                                   

/*pruned*/;                                                                   
          
                    
                    
                    
          
          
                    
                    
          
          
          
                    
          
          
          
          
 


/*pruned*/;                                                                   
                     
                     
                     
           
 

/*pruned*/;          
                                   
 

/*pruned*/;                                                   
          
          
          
          
          
          
          
          
          
 

type Foo = {
    a: number;
    b: number | undefined;
    c?: number;
    d?: number | undefined;
}

let x1: Foo = null as unknown as (Foo);

x1.a;  // number
x1.b;  // number | undefined
x1.c;  // number | undefined
x1.d;  // number | undefined

x1 = { a: 1 };  // Error
x1 = { a: 1, b: 1 };
x1 = { a: 1, b: 1, c: 1 };
x1 = { a: 1, b: 1, c: 1, d: 1 };

let x2: Required<Foo> = null as unknown as (Required<Foo>);

x1.a;  // number
x1.b;  // number | undefined
x1.c;  // number
x1.d;  // number

x2 = { a: 1 };  // Error
x2 = { a: 1, b: 1 };  // Error
x2 = { a: 1, b: 1, c: 1 };  // Error
x2 = { a: 1, b: 1, c: 1, d: 1 };

type Bar = {
    a: number;
    readonly b: number;
}

let x3: Bar = null as unknown as (Bar);
x3.a = 1;
x3.b = 1;  // Error

let x4: Readonly<Bar> = null as unknown as (Readonly<Bar>);
x4.a = 1;  // Error
x4.b = 1;  // Error

/*pruned*/;                                                  
/**/;    
/**/;    


function main(): void {}
