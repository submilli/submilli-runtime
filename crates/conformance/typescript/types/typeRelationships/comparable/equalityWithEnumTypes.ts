// @target: es2015
// @strict: true

// Literal enum type
enum E1 {
    a = 1,
    b = 2,
}

// Numeric enum type
/**/;    
               
              
 

function f1(v: E1): void {
    if (v !== 0) {  // Error
        v;
    }
    if (v !== 1) {
        v;
    }
    if (v !== 2) {
        v;
    }
    if (v !== 3) {  // Error
        v;
    }
}

/*pruned*/;               
                  
          
     
                  
          
     
                  
          
     
                  
          
     
 


function main(): void {}
