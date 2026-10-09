// @strict: true
// @declaration: true
// @target: es2015
// @lib: esnext, dom

type Action =
    | { kind: 'A', payload: number }
    | { kind: 'B', payload: string };

function f10({ kind, payload }: Action): void {
    if (kind === 'A') {
        payload.toFixed();
    }
    if (kind === 'B') {
        payload.toUpperCase();
    }
}

function f11(action: Action): void {
    const { kind, payload } = action;
    if (kind === 'A') {
        payload.toFixed();
    }
    if (kind === 'B') {
        payload.toUpperCase();
    }
}

function f12({ kind, payload }: Action): void {
    switch (kind) {
        case 'A':
            payload.toFixed();
            break;
        case 'B':
            payload.toUpperCase();
            break;
        default:
            payload;  // never
    }
}

// repro #50206
/*pruned*/;                                                 
                       
                          
     
                       
                              
     
 

/*pruned*/;                                 
                                
                       
                          
     
                       
                              
     
 

type Action2 =
    | { kind: 'A', payload: number | undefined }
    | { kind: 'B', payload: string | undefined };

function f20({ kind, payload }: Action2): void {
    if (payload) {
        if (kind === 'A') {
            payload.toFixed();
        }
        if (kind === 'B') {
            payload.toUpperCase();
        }
    }
}

function f21(action: Action2): void {
    const { kind, payload } = action;
    if (payload) {
        if (kind === 'A') {
            payload.toFixed();
        }
        if (kind === 'B') {
            payload.toUpperCase();
        }
    }
}

function f22(action: Action2): void {
    if (action.payload) {
        const { kind, payload } = action;
        if (kind === 'A') {
            payload.toFixed();
        }
        if (kind === 'B') {
            payload.toUpperCase();
        }
    }
}

function f23({ kind, payload }: Action2): void {
    if (payload) {
        switch (kind) {
            case 'A':
                payload.toFixed();
                break;
            case 'B':
                payload.toUpperCase();
                break;
            default:
                payload;  // never
        }
    }
}

type Foo =
    | { kind: 'A', isA: true }
    | { kind: 'B', isA: false }
    | { kind: 'C', isA: false };

function f30({ kind, isA }: Foo): void {
    if (kind === 'A') {
        isA;   // true
    }
    if (kind === 'B') {
        isA;   // false
    }
    if (kind === 'C') {
        isA;   // false
    }
    if (isA) {
        kind;  // 'A'
    }
    else {
        kind;  // 'B' | 'C'
    }
}

type Args = ['A', number] | ['B', string]

/*pruned*/;                                
                       
                       
     
                       
                           
     
 

// Repro from #35283

interface A<T> { variant: 'a', value: T }

interface B<T> { variant: 'b', value: Array<T> }

type AB<T> = A<T> | B<T>;

function printValue<T>(t: T): void { }

function printValueList<T>(t: Array<T>): void { }

function unrefined1<T>(ab: AB<T>): void {
    const { variant, value } = ab;
    if (variant === 'a') {
        printValue<T>(value);
    }
    else {
        printValueList<T>(value);
    }
}

// Repro from #38020

type Action3 =
    | {type: 'add', payload: { toAdd: number } }
    | {type: 'remove', payload: { toRemove: number } };

const reducerBroken = (state: number, { type, payload }: Action3) => {
    switch (type) {
        case 'add':
            return state + payload.toAdd;
        case 'remove':
            return state - payload.toRemove;
    }
}

// Repro from #46143

/*pruned*/;                                                      
/*pruned*/;                       
/*pruned*/; 
                     
 

// Repro from #46658

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

// repro from https://github.com/microsoft/TypeScript/pull/47190#issuecomment-1057603588

/*pruned*/;       
                 
                                            
                                          
          
 

/*pruned*/;            
                    
                        
             
            
               
     
   
  

/*pruned*/;            
                 
                                            
                                          
                  
 

/*pruned*/;                      
                          
                        
             
            
               
     
   
  

/*pruned*/;          
                 
                                            
                                          
                              
 

/*pruned*/;                  
                     
                        
             
            
               
     
   
  

/*pruned*/;               
                 
                                            
                                          
                                   
 

/*pruned*/;                            
                           
                        
             
            
               
     
   
  

// Repro from #48345

/*pruned*/;                                                               

/*pruned*/;                           
                       
                                    
     
                       
                                        
     
  

// Repro from #48902

/*pruned*/;   
           
                         
                         
                         
                         
                         
                         
                         
                         
                        
           

// Repro from #49772

function fa1(x: [true, number] | [false, string]): void {
    const [guard, value] = x;
    if (guard) {
        for (;;) {
            value;  // number
        }
    }
    else {
        while (!!true) {
            value;  // string
        }
    }
}

function fa2(x: { guard: true, value: number } | { guard: false, value: string }): void {
    const { guard, value } = x;
    if (guard) {
        for (;;) {
            value;  // number
        }
    }
    else {
        while (!!true) {
            value;  // string
        }
    }
}

/*pruned*/;                                                                         
                
                  
                             
         
     
          
                        
                             
         
     
 

// Repro from #52152

/*pruned*/;             
                            
                                                               
 
  
/*pruned*/;           
                                                                                                          
 

/*pruned*/;              
/*pruned*/;                                                                                                                                
/*pruned*/;                                                                                        

// Destructuring tuple types with different arities

function fz1([x, y]: [1, 2] | [3, 4] | [5]): void {
    if (y === 2) {
        x;  // 1
    }
    if (y === 4) {
        x;  // 3
    }
    if (y === undefined) {
        x;  // 5
    }
    if (x === 1) {
        y;  // 2
    }
    if (x === 3) {
        y;  // 4
    }
    if (x === 5) {
        y;  // undefined
    }
}

// Repro from #55661

function tooNarrow([x, y]: [1, 1] | [1, 2] | [1]): void {
    if (y === undefined) {
        const shouldNotBeOk: never = x;  // Error
    }
}

// https://github.com/microsoft/TypeScript/issues/56312

function parameterReassigned1([x, y]: [1, 2] | [3, 4]): void {
  if (Math.random()) {
    x = 1;
  }
  if (y === 2) {
    x; // 1 | 3
  }
}

function parameterReassigned2([x, y]: [1, 2] | [3, 4]): void {
  if (Math.random()) {
    y = 2;
  }
  if (y === 2) {
    x; // 1 | 3
  }
}

// https://github.com/microsoft/TypeScript/pull/56313#discussion_r1416482490

/*pruned*/;                                                                               
                      
          
   
                
               
   
 


function main(): void {}
