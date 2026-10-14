// @target: es2015
let x: number = null as unknown as (number);
x = 3; // OK
(x) = 3; // OK
x = ''; // Error
(x) = ''; // Error

/*pruned*/;  
                                                
 
/**/;    // OK
/**/;      // OK
/**/;      // OK
/**/;     // Error
/*pruned*/; // Error
/*pruned*/; // Error

/*pruned*/;   // Error
/*pruned*/;     // Error

/*pruned*/;   
                         
                                                    
     

                           
 
/*pruned*/;       // OK
/*pruned*/;         // OK
/*pruned*/;         // OK

/*pruned*/;        // Error
/*pruned*/;          // Error
/*pruned*/;          // Error


function fn(): void { }
fn = () => 3; // Bug 823548: Should be error (fn is not a reference)
(fn) = () => 3; // Should be error

function fn2(x: number, y: { t: number }): void {
    x = 3;
    (x) = 3; // OK
    x = ''; // Error
    (x) = ''; // Error

    (y).t = 3; // OK
    (y.t) = 3; // OK
    (y).t = ''; // Error
    (y.t) = ''; // Error

    y['t'] = 3; // OK
    (y)['t'] = 3; // OK
    (y['t']) = 3; // OK
    y['t'] = ''; // Error
    (y)['t'] = ''; // Error
    (y['t']) = ''; // Error
}

enum E {
    A
}
E = undefined; // Error
(E) = undefined; // Error

class C {

}

C = undefined; // Error
(C) = undefined; // Error


function main(): void {}
