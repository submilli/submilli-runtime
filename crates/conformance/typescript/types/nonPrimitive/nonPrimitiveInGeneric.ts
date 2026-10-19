// @target: es2015
// @declaration: true
function generic<T>(t: T): void {
    let o: object = t; // expect error
}
let a = {};
let b = "42";

generic<object>({});
generic<object>(a);
generic<object>(123); // expect error
generic<object>(b); // expect error

/*pruned*/;                                   
                            
 

/**/;     
/**/;    
/*pruned*/; // expect error
/**/;     // expect error

/*pruned*/;                                 

/*pruned*/;  
/*pruned*/;      
/*pruned*/;       // expect error
/*pruned*/;       // expect error

/*pruned*/;                                
                            
 

/*pruned*/;                         

/*pruned*/;                                                // error
/*pruned*/;                                            // ok
/*pruned*/;                                                       // ok


interface Blah {
    foo: number;
}

/*pruned*/;                                            // ok


function main(): void {}
