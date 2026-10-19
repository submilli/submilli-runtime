// @target: es2015
// @strict: true
// ArrowFormalParameters => AssignmentExpression is equivalent to ArrowFormalParameters => { return AssignmentExpression; }
let a = (p: string) => p.length;
let a_2 = (p: string) => { return p.length; }

// Identifier => Block is equivalent to(Identifier) => Block
/*pruned*/;               
/*pruned*/;                   

// Identifier => AssignmentExpression is equivalent to(Identifier) => AssignmentExpression
let c: number = null as unknown as (number);
/*pruned*/;        
/*pruned*/;            
/*pruned*/;                                                     

// Binding patterns in arrow functions
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

// Arrow function used in class member initializer
// Arrow function used in class member function
/*pruned*/;    
                     
                         

                
                             
                                 
     
 

// Arrow function used in arrow function
let arrrr = () => (m: number) => () => (n: number) => m + n;
let e = arrrr()(3)()(4);
let e_2: number = null as unknown as (number);

// Arrow function used in arrow function used in function
function someFn(): void {
    let arr = (n: number) => (p: number) => p * n;
    arr(3)(4).toExponential();
}

// Arrow function used in function
function someOtherFn(): void {
    let arr = (n: number) => '' + n;
    arr(4).charAt(0);
}

// Arrow function used in nested function in function
function outerFn(): void {
    function innerFn(): void {
        let arrowFn = () => { };
        let p = arrowFn();
        let p_2: void = null as unknown as (void);
    }
}

// Arrow function used in nested function in arrow function
let f = (n: string) => {
    function fn(x: number): () => string {
        return () => n + x;
    }
    return fn(4);
}
let g = f('')();
let g_2: string = null as unknown as (string);


// Arrow function used in nested function in arrow function in nested function
function someOuterFn(): (n: string) => () => () => number {
    let arr = (n: string) => {
        function innerFn(): () => number {
            return () => n.length;
        }
        return innerFn;
    }
    return arr;
}
let h = someOuterFn()('')()();
h.toExponential();

// Arrow function used in try/catch/finally in function
function tryCatchFn(): void {
    try {
        /*pruned*/;        
    } catch (e) {
        /*pruned*/;            
    } finally {
        /*pruned*/;             
    }
}


function main(): void {}
