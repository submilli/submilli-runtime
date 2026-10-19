// @target: es5, es2015
// @strict: true
// @allowUnreachableCode: true

// FunctionExpression with no return type annotation with multiple return statements with unrelated types
let f1 = function () {
    return '';
    return 3;
};
let f2 = function x() {
    return '';
    return 3;
};
let f3 = () => {
    return '';
    return 3;
};

// FunctionExpression with no return type annotation with return branch of number[] and other of string[]
let f4 = function () {
    if (true) {
        return [''];
    } else {
        return [1];
    }
}

// Function implemetnation with non -void return type annotation with no return
function f5(): number {
}

/*pruned*/;                           
// Function signature with parameter initializer referencing in scope local variable
/*pruned*/;               
              
 

// Function signature with initializer referencing other parameter to the right
/*pruned*/;                   
 

// FunctionExpression with non -void return type annotation with a throw, no return, and other code
// Should be error but isn't
undefined === function (): number {
    throw undefined;
    let x = 4;
};

/*pruned*/;              
/*pruned*/;                      
/*pruned*/;                               
/*pruned*/;                               
/*pruned*/;                         
                          
                              
 
/*pruned*/;           
                          
                          
  
/*pruned*/;      
                          
                          
  
/*pruned*/;                          
                      
                              
 
/*pruned*/;            
                      
                              
  
/*pruned*/;      
                      
                              
  


function main(): void {}
