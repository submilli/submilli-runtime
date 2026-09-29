// @target: es2015
// @strict: true
//When used as a contextual type, a union type U has those members that are present in any of 
// its constituent types, with types that are unions of the respective members in the constituent types. 
interface I1<T> {
    commonMethodType(a: string): string;
    commonPropertyType: string;
    commonMethodWithTypeParameter(a: T): T;

    methodOnlyInI1(a: string): string;
    propertyOnlyInI1: string;
}
interface I2<T> {
    commonMethodType(a: string): string;
    commonPropertyType: string;
    commonMethodWithTypeParameter(a: T): T;

    methodOnlyInI2(a: string): string;
    propertyOnlyInI2: string;
}

// Let S be the set of types in U that has a property P.
// If S is not empty, U has a property P of a union type of the types of P from each type in S.
/*pruned*/;                                          
/*pruned*/;                                          
/*pruned*/;                              
/*pruned*/;                                
let i1Ori2_3: I1<number> | I2<number> = { // Like i1
    commonPropertyType: "hello",
    commonMethodType: a=> a,
    commonMethodWithTypeParameter: a => a,

    methodOnlyInI1: a => a,
    propertyOnlyInI1: "Hello",
};
let i1Ori2_4: I1<number> | I2<number> = { // Like i2
    commonPropertyType: "hello",
    commonMethodType: a=> a,
    commonMethodWithTypeParameter: a => a,

    methodOnlyInI2: a => a,
    propertyOnlyInI2: "Hello",
};
/*pruned*/;                                                     
                                
                            
                                          
                           
                              
                           
                              
  

/*pruned*/;                                                            
                                    
                                
                                              

                               
                                  
      
                
                                    
                                
                                              

                               
                                  
                               
                                    
                                
                                              
                               
                                  
                               
                                  
       

interface I11 {
    commonMethodDifferentReturnType(a: string, b: number): string;
    commonPropertyDifferentType: string;
}
interface I21 {
    commonMethodDifferentReturnType(a: string, b: number): number;
    commonPropertyDifferentType: number;
}
/*pruned*/;                             
/*pruned*/;                             
/*pruned*/;                   
/*pruned*/;                     
/*pruned*/;                   
              
                                                
                            
                   
      
                                           
  
/*pruned*/;                   
              
                                                
                                
                 
      
                                    
  
/*pruned*/;                                                      
                  
                                                    
                                
                     
          
                                             
         
                  
                                                    
                                    
                     
          
                                        
       

function main(): void {}
