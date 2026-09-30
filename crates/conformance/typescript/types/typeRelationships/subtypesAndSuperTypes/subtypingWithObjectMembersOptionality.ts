// @target: es2015
// Derived member is not optional but base member is, should be ok

interface Base { foo: string; }
interface Derived extends Base { bar: string; }
interface Derived2 extends Derived { baz: string; }

// S is a subtype of a type T, and T is a supertype of S, if one of the following is true, where S' denotes the apparent type (section 3.8.1) of S:
//   - S' and T are object types and, for each member M in T, one of the following is true:
//      - M is a property and S' contains a property N where
//          M and N have the same name,
//          the type of N is a subtype of that of M,
//          M and N are both public or both private, and
//          if M is a required property, N is also a required property.
//      - M is an optional property and S' contains no property of the same name as M.
interface T {
    Foo?: Base;
}

interface S extends T {
    Foo: Derived
}

/*pruned*/;   
              
 

/*pruned*/;              
               
 

interface T3 {
    '1'?: Base;
}

interface S3 extends T3 {
    '1.': Derived;
}

// object literal case
let a: { Foo?: Base; } = null as unknown as ({ Foo?: Base; });
let b = { Foo: <Derived>null };
let r = true ? a : b;

/*pruned*/;          
                 
                   
     

                           
                      
     

                  
                 
     

                             
                    
     

                  
                   
     

                             
                       
     

                          
                                                                  
                                    
                         
 

function main(): void {}
