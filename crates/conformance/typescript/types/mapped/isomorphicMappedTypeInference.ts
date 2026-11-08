// @target: es2015
// @strictNullChecks: true
// @noimplicitany: true
// @declaration: true

type Box<T> = {
    value: T;
}

/*pruned*/;         
                              
 

function box<T>(x: T): Box<T> {
    return { value: x };
}

function unbox<T>(x: Box<T>): T {
    return x.value;
}

/*pruned*/;                              
                                   
                        
                                
     
                  
 

/*pruned*/;                                               
                         
                        
                                  
     
                  
 

/*pruned*/;                                                    
                           
                                 
     
 

function f1(): void {
    let v = {
        a: 42,
        b: "hello",
        c: true
    };
    /*pruned*/;       
    /*pruned*/;               
}

function f2(): void {
    let b = {
        a: box(42),
        b: box("hello"),
        c: box(true)
    };
    /*pruned*/;         
    /*pruned*/;         
}

function f3(): void {
    let b = {
        a: box(42),
        b: box("hello"),
        c: box(true)
    };
    /*pruned*/;                     
}

function f4(): void {
    let b = {
        a: box(42),
        b: box("hello"),
        c: box(true)
    };
    /*pruned*/;             
    /*pruned*/;             
}

/*pruned*/;                                                                       
               
 

function f5(s: string): void {
    /*pruned*/;         
                   
                        
                    
       
    /*pruned*/;         
    /*pruned*/;                            
}

function makeDictionary<T>(obj: { [x: string]: T }): { [x: string]: T; } {
    return obj;
}

function f6(s: string): void {
    let b = makeDictionary({
        a: box(42),
        b: box("hello"),
        c: box(true)
    });
    /*pruned*/;         
    /*pruned*/;                             
}

/*pruned*/;                                                                               
/*pruned*/;                                                                                    
/*pruned*/;                                                                                                

type Foo = {
    a?: number;
    readonly b: string;
}

function f10(foo: Foo): void {
    /*pruned*/;             // { a: number, readonly b: string }
    /*pruned*/;          // { a?: number, b: string }
    /*pruned*/;                     // { a: number, b: string }
}

// Repro from #12606

/*pruned*/;                          
/*pruned*/;     
                                             
  

/**
 * Given a spec object recursively mapping properties to functions, creates a function
 * producing an object of the same structure, by mapping each property to the result
 * of calling its associated function with the supplied arguments.
 */
/*pruned*/;                                                                                                      

// Infers g1: (...args: any[]) => { sum: number, nested: { mul: string } }
/*pruned*/;         
                       
             
                            
     
   

// Infers g2: (...args: any[]) => { foo: { bar: { baz: boolean } } }
/*pruned*/;                                                     

// Repro from #12633

/*pruned*/;                                               
let o = {a: 5, b: 7};
/*pruned*/;    
/*pruned*/;        

// Inferring to { [P in K]: X }, where K extends keyof T, produces same inferences as
// inferring to { [P in keyof T]: X }.

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

// Repro from #29765

/*pruned*/;                                                             
                     
 

/*pruned*/;           

/*pruned*/;                                

/*pruned*/;                                                        


function main(): void {}
