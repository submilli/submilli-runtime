// @target: es2015
// @strict: true
// Optional parameters should be valid in all the below casts

function foo(x?: number): void { }
let f = function foo(x?: number) { }
let f2 = (x: number, y?: number) => { }

foo(1);
foo();
f(1);
f();
f2(1);
f2(1, 2);

class C {
    foo(x?: number): void { }
}

/*pruned*/;                       
/**/;   
/**/;    

/*pruned*/;  
                 
                               
 

/*pruned*/;                       
;   
/**/;
/**/;    
/*pruned*/; 

/**/;   
                 
                    
                         
                 
                    
   

;   
/**/;
/**/;   
/**/;    

let b = {
    foo(x?: number) { },
    a: function foo(x: number, y?: number) { },
    b: (x?: number) => { }
}

b.foo();
b.foo(1);
b.a(1);
b.a(1, 2);
b.b();
b.b(1);


function main(): void {}
