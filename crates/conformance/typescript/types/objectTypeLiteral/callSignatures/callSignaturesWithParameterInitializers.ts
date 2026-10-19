// @target: es2015
// @strict: true
// Optional parameters allow initializers only in implementation signatures

function foo(x: number = 1): void { }
let f = function foo(x: number = 1) { }
let f2 = (x: number, y: number = 1) => { }

foo(1);
foo();
f(1);
f();
f2(1);
f2(1, 2);

class C {
    foo(x: number = 1): void { }
}

/*pruned*/;                       
/**/;   
/**/;    

// these are errors
/*pruned*/;  
                    
                                  
 

/*pruned*/;                       
;   
/**/;
/**/;    
/*pruned*/; 

// these are errors
/**/;   
                    
                       
                         
                    
                       
   

;   
/**/;
/**/;   
/**/;    

let b = {
    foo(x: number = 1) { },
    a: function foo(x: number, y: number = 1) { },
    b: (x: number = 1) => { }
}

b.foo();
b.foo(1);
b.a(1);
b.a(1, 2);
b.b();
b.b(1);


function main(): void {}
