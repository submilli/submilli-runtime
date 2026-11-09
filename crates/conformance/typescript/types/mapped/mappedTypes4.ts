// @target: es2015
// @strictNullChecks: true
// @declaration: true

type Box<T> = {
};

/*pruned*/;         
                              
  

/*pruned*/;                              
                                  
                                       
                            
                                          
         
                      
     
                    
 

type A = { a: string };
type B = { b: string };
type C = { c: string };

/*pruned*/;                                                             
                     
 

type T00 = Partial<A | B | C>;
type T01 = Readonly<A | B | C | null | undefined>;
/*pruned*/;                              
type T03 = Readonly<string | number | boolean | null | undefined | void>;
/*pruned*/;                                                              
type T05 = Partial<"hello" | "world" | 42>;

/*pruned*/;                        
                                  
 

/*pruned*/;                                      
/*pruned*/;                                           
/*pruned*/;                                        

/*pruned*/;             
                                                
  

type Foo = {
    x: number;
    y: { a: string, b: number };
    z: boolean;
};

type DeepReadonlyFoo = {
    readonly x: number;
    readonly y: { readonly a: string, readonly b: number };
    readonly z: boolean;
};

/*pruned*/;                                                        
let x1_2: DeepReadonlyFoo = null as unknown as (DeepReadonlyFoo);

// Repro from #13232

type Z = { a: number };
/*pruned*/;      
                                     
  
/*pruned*/;        // M should be { a: number }

let z1: Z = null as unknown as (Z);
/*pruned*/;                                        


function main(): void {}
