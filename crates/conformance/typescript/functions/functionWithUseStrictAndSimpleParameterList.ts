// @target: es2015
// @strict: true
/*pruned*/;                       
                 
 

export let foo = 10;
function b(a: number = 10): void {
}

function container(): void {
    "use strict";
    function f(a: number = 10): void {
    }
}

/*pruned*/;                          
                 
 

/*pruned*/;                                   
                 
 

/*pruned*/;                                     
                 
 

/*pruned*/;                                      
                 
 

/*pruned*/;                                       
                 
 

/*pruned*/;                                                   
                 
 

/*pruned*/;                                                  
          
                 
 

function invalidPrologue(a: number = 10, b: number = 20): void {
    "foo";
    const c = 1;
    "use strict";
}


function main(): void {}
