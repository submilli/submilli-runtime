// @target: es2015
// @strict: true
// Enum without initializers have first member = 0 and successive members = N + 1
enum E1 {
    A,
    B,
    C
}

// Enum type is a subtype of Number
let x: number = E1.A;

// Enum object type is anonymous with properties of the enum type and numeric indexer
/*pruned*/;
/**/;     
                     
                     
                     
                                 
                         
                     
                     
                     
                                 
   
/*pruned*/;                                         

// Reverse mapping of enum returns string name of property 
/*pruned*/;     
let s_2: string = null as unknown as (string);


// Enum with only constant members
enum E2 {
    A = 1, B = 2, C = 3
}

// Enum with only computed members
/**/;    
                                           
 

// Enum with constant members followed by computed members
/**/;    
                              
 

// Enum with > 2 constant members with no initializer for first member, non zero initializer for second element
enum E5 {
    A,
    B = 3,
    C // 4
}

enum E6 {
    A,
    B = 0,
    C // 1
}

// Enum with computed member initializer of type 'any'
/**/;    
                    
 

// Enum with computed member initializer of type number
/**/;    
                    
 

//Enum with computed member intializer of same enum type
/**/;    
      
         
 

// (refer to .js to validate)
// Enum constant members are propagated
/*pruned*/;           
                                      
  
// Enum computed members are not propagated
/*pruned*/;        
                                                  
  



function main(): void {}
