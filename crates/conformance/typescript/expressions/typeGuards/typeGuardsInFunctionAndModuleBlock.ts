// @target: es2015
// typeguards are scoped in function/module block

function foo(x: number | string | boolean): string {
    return typeof x === "string"
        ? x
        : function f() {
            let b = x; // number | boolean
            return typeof x === "boolean"
                ? x.toString() // boolean
                : x.toString(); // number
        } ();
}
function foo2(x: number | string | boolean): string {
    return typeof x === "string"
        ? x
        : function f(a: number | boolean) {
            let b = x; // new scope - number | boolean
            return typeof x === "boolean"
                ? x.toString() // boolean
                : x.toString(); // number
        } (x); // x here is narrowed to number | boolean
}
function foo3(x: number | string | boolean): string {
    return typeof x === "string"
        ? x
        : (() => {
            let b = x; // new scope - number | boolean
            return typeof x === "boolean"
                ? x.toString() // boolean
                : x.toString(); // number
        })();
}
function foo4(x: number | string | boolean): string {
    return typeof x === "string"
        ? x
        : ((a: number | boolean) => {
            let b = x; // new scope - number | boolean
            return typeof x === "boolean"
                ? x.toString() // boolean
                : x.toString(); // number
        })(x); // x here is narrowed to number | boolean
}
// Type guards do not affect nested function declarations
function foo5(x: number | string | boolean): void {
    if (typeof x === "string") {
        let y = x; // string;
        /*pruned*/;           
                                
         
    }
}
/*pruned*/;  
                                                                                      
                  
                                                           
                                                    
                                    
                            
                
                                      
                                     
                                     
         
     
 
/*pruned*/;   
                                                                                      
                     
                                                           
                                                    
                                    
                            
                
                                      
                                     
                                     
         
     
 

function main(): void {}
