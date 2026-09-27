// @target: es2015
function f1(): void {
    /*pruned*/;                               
    /**/; 
    /**/;     
    /**/;     
    /**/;    
    /**/;    
    /**/;  
    ;   
}

function f2(): never {
    return;
}

function f3(): never {
    return 1;
}

function f4(): never {
}

for (const n of f4()) {}
/*pruned*/;             

function f5(): void {
    let x: never[] = [];  // Ok
}

// Repro from #46032

interface A {
    foo: "a";
}

interface B {
    foo: "b";
}

/*pruned*/;        

/*pruned*/;                          
            
                  
      
 


function main(): void {}
