// @target: es2015
// @strict: true
// @declaration: true

/*pruned*/;              // {} & string
/*pruned*/;           // 'a'
/*pruned*/;              // object
/*pruned*/;                     // { x: number }
/*pruned*/;            // never
/*pruned*/;            // never
/*pruned*/;              // undefined

/*pruned*/;              // Specially preserved
/*pruned*/;              // Specially preserved
/*pruned*/;              // Specially preserved

type ThisNode = {};
type ThatNode = {};
type ThisOrThatNode = ThisNode | ThatNode;

function f01(u: unknown): void {
    let x1: {} = u;  // Error
    let x2: {} | null | null = u;
    let x3: {} | { x: string } | null | null = u;
    let x4: ThisOrThatNode | null | null = u;
}

function f10(x: unknown): void {
    if (x) {
        x;  // {}
    }
    else {
        x;  // unknown
    }
    if (!x) {
        x;  // unknown
    }
    else {
        x;  // {}
    }
}

function f11<T>(x: T): void {
    if (x) {
        x;  // T & {}
    }
    else {
        x;  // T
    }
    if (!x) {
        x;  // T
    }
    else {
        x;  // T & {}
    }
}

/*pruned*/;                             
            
                
     
          
                
     
 

function f20(x: unknown): void {
    if (x !== null) {
        x;  // {} | null
    }
    else {
        x;  // undefined
    }
    if (x !== null) {
        x;  // {} | undefined
    }
    else {
        x;  // null
    }
    if (x !== null && x !== null) {
        x;  // {}
    }
    else {
        x;  // null | undefined
    }
    if (x != null) {
        x;  // {}
    }
    else {
        x;  // null | undefined
    }
    if (x != null) {
        x;  // {}
    }
    else {
        x;  // null | undefined
    }
}

function f21<T>(x: T): void {
    if (x !== null) {
        x;  // T & ({} | null)
    }
    else {
        x;  // T
    }
    if (x !== null) {
        x;  // T & ({} | undefined)
    }
    else {
        x;  // T
    }
    if (x !== null && x !== null) {
        x;  // T & {}
    }
    else {
        x;  // T
    }
    if (x != null) {
        x;  // T & {}
    }
    else {
        x;  // T
    }
    if (x != null) {
        x;  // T & {}
    }
    else {
        x;  // T
    }
}

/*pruned*/;                                    
                     
                     
     
          
                
     
                     
                
     
          
                
     
                                   
                     
     
          
                
     
                    
                     
     
          
                
     
                    
                     
     
          
                
     
 

function f23<T>(x: T | null | null): void {
    if (x !== null) {
        x;  // T & {} | null
    }
    if (x !== null) {
        x;  // T & {} | undefined
    }
    if (x != null) {
        x;  // T & {}
    }
    if (x != null) {
        x;  // T & {}
    }
}

function f30(x: {}): void {
    if (typeof x === "object") {
        x;  // object
    }
}

function f31<T>(x: T): void {
    if (typeof x === "object") {
        x;  // T & object | T & null
    }
    if (x && typeof x === "object") {
        x;  // T & object
    }
    if (typeof x === "object" && x) {
        x;  // T & object
    }
}

/*pruned*/;                                    
                                
                         
     
 

function possiblyNull<T>(x: T): T | null {
    return !!true ? x : null;  // T | null
}

function possiblyUndefined<T>(x: T): T | null {
    return !!true ? x : null;  // T | undefined
}

function possiblyNullOrUndefined<T>(x: T): T | null {
    return possiblyUndefined(possiblyNull(x));  // T | null | undefined
}

/*pruned*/;                                       
                                  
                                      
 

/*pruned*/;                                            
                                  
                                 
 

/*pruned*/;                                                        
                                                           
 

function f40(a: string | null, b: number | null | null): void {
    /*pruned*/;                            // string
    /*pruned*/;                            // number
}

/*pruned*/;                                           

function f41<T>(a: T): void {
    /*pruned*/;                                     // T & {}
    /*pruned*/;                                     // T & {}
    /*pruned*/;                                // T & {} | T & undefined
    /*pruned*/;                                          // T & {} | T & null
    /*pruned*/;                                                      // T & {}
    /*pruned*/;                                          // T & {} | undefined
    /*pruned*/;                                               // T & {} | null
    /*pruned*/;                                    // T & {} | undefined
    /*pruned*/;                                    // T & {} | null
}

// Repro from #48468

function deepEquals<T>(a: T, b: T): boolean {
    if (typeof a !== 'object' || typeof b !== 'object' || !a || !b) {
        return false;
    }
    if (Array.isArray(a) || Array.isArray(b)) {
        return false;
    }
    if (Object.keys(a).length !== Object.keys(b).length) { // Error here
        return false;
    }
    return true;
}

// Repro from #49386

function foo<T>(x: T | null): void {
    let y = x;
    if (y !== null) {
        y;
    }
}

// We allow an unconstrained object of a generic type `T` to be indexed by a key of type `keyof T`
// without a check that the object is non-undefined and non-null. This is safe because `keyof T`
// is `never` (meaning no possible keys) for any `T` that includes `undefined` or `null`.

function ff1<T>(t: T, k: keyof T): void {
    t[k];
}

/*pruned*/;                                   
         
 

/*pruned*/;                                     
                   
 

/*pruned*/;                                          
         
 

ff1(null, 'foo');  // Error
/*pruned*/;        // Error
/*pruned*/;      
/*pruned*/;        // Error

// Repro from #49681

/*pruned*/;                           
/*pruned*/;                   

/*pruned*/;                                              

// Generics and intersections with {}

/*pruned*/;                                    
                       
                         
     
          
                                  
     
 

/*pruned*/;                                                    
                       
                         
     
          
                                  
     
 

/*pruned*/;                                               
                       
                         
     
          
                                  
     
 

/*pruned*/;                                                      
                       
                         
     
          
                                  
     
 

/*pruned*/;                                                      
                       
                         
     
          
                                  
     
 

/*pruned*/;                                                             
                       
                         
     
          
                                  
     
 

// Double-equals narrowing

function fx10(x: string | number, y: number): void {
    if (x == y) {
        x;  // string | number
    }
    else {
        x;  // string | number
    }
    if (x != y) {
        x;  // string | number
    }
    else {
        x;  // string | number
    }
}

// Repros from #50706

function SendBlob(encoding: unknown): void {
    if (encoding !== null && encoding !== 'utf8') {
        throw new Error('encoding');
    }
    encoding;
};

/*pruned*/;                                            
                         
                     
     
                       
                                             
     
                 
 

function doSomething2(value: unknown): void {
    if (value === null) {
        return;
    }
    if (value === 42) {
        value;
    }
}

// Repro from #51009

type TypeA = {
    A: 'A',
    B: 'B',
}

type TypeB = {
    A: 'A',
    B: 'B',
    C: 'C',
}

/*pruned*/;                    
                                                         

/*pruned*/;                     
                                                                                         

// Repro from #51041

type AB = "A" | "B";

/*pruned*/;                                                
                                     
  

// Repro from #51538

type Left = 'left';
/*pruned*/;                               
/*pruned*/;                

function assertNever(v: never): never {
    throw new Error('never');
}

/*pruned*/;                         
                           
                                  
     
                                 
                                   
     
          
                           
     
 


function main(): void {}
