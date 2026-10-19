// @target: es5, es2015
// @strict: true
/*pruned*/;                           
      
      
 

/*pruned*/;                         
      
      
 

/*pruned*/;                                 
      
      
      
 

function inside(a = b): void {
    /**/; 
}

function outside(): void {
    /**/; 
    /*pruned*/;                                                                                          
              
     
}

function defaultArgFunction(a: () => number = function () { return b; }, b: number = 1): void { }
function defaultArgArrow(a: () => () => number = () => () => b, b: number = 3): void { }

class C {
    constructor(a: number = b, b: number = 1) { }
    method(a: number = b, b: number = 1): void { }
}

// Function expressions
let x = (a = b, b = c, c = d) => { /**/;  };

// Should not produce errors - can reference later parameters if they occur within a function expression initializer.
/*pruned*/;                                                  
 

function main(): void {}
