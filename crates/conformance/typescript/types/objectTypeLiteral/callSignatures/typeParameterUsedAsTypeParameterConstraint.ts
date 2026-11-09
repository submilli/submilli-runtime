// @target: es2015
// Type parameters are in scope in their own and other type parameter lists

function foo<T, U extends T>(x: T, y: U): T {
    x = y;
    return y;
}

function foo2<U extends T, T>(x: T, y: U): T {
    x = y;
    return y;
}

/*pruned*/;                                       
          
             
 

/*pruned*/;                                        
          
             
 

/*pruned*/;                                  
          
             
 

/*pruned*/;                                  
          
             
 

function main(): void {}
