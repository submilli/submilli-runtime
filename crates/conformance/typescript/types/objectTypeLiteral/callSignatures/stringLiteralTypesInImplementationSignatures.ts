// @target: es2015
// @strict: true
// String literal types are only valid in overload signatures

function foo(x: 'hi'): void { }
let f = function foo(x: 'hi') { }
let f2 = (x: 'hi', y: 'hi') => { }

class C {
    foo(x: 'hi'): void { }
}

/*pruned*/;  
              
                          
 

/**/;   
              
                 
                         
              
                 
   

let b = {
    foo(x: 'hi') { },
    a: function foo(x: 'hi', y: 'hi') { },
    b: (x: 'hi') => { }
}


function main(): void {}
