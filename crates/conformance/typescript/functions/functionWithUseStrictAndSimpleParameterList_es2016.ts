// @strict: true
// @target: es2016

function a(a: number = 10): void {
    "use strict";
}

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
                 
 

function paramDefault(param: number = 1): void {
    'use strict';
}

/*pruned*/;                                      
                 
 

/*pruned*/;                                       
                 
 

function manyParameter(a: number = 10, b: number = 20): void {
    "use strict";
}

function manyPrologue(a: number = 10, b: number = 20): void {
    "foo";
    "use strict";
}

function invalidPrologue(a: number = 10, b: number = 20): void {
    "foo";
    const c = 1;
    "use strict";
}


function main(): void {}
