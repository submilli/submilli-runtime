// @target: es2015
enum E {
    A, B, C
}

let cond: boolean = null as unknown as (boolean);

/*pruned*/;                                                 
               
                  
                   
                   
                
                           
                  
                 
               
                  
                   
                   
                
                           
                  
                 
                 
                    
                     
                     
                  
                             
                    
                   
 

/*pruned*/;                                                                      
                  
                        
                          
                        
                      
                  
                        
                          
                        
                      
 

function f3(): void {
    const c1 = cond ? 1 : 2;
    const c2 = cond ? 1 : "two";
    const c3 = cond ? E.A : cond ? true : 123;
    const c4 = cond ? "abc" : null;
    const c5 = cond ? 456 : null;
    const c6: { kind: 123 } = { kind: 123 };
    const c7: [1 | 2, "foo" | "bar"] = [1, "bar"];
    const c8 = cond ? c6 : cond ? c7 : "hello";
    let x1 = c1;
    let x2 = c2;
    let x3 = c3;
    let x4 = c4;
    let x5 = c5;
    let x6 = c6;
    let x7 = c7;
    let x8 = c8;
}

/**/;     
           
              
               
               
            
                       
              
             
                    
                       
                        
                        
                     
                                
                       
                      
 

function f4(): void {
    const c1 = { a: 1, b: "foo" };
    const c2: { a : 0 | 1, b: "foo" | "bar" } = { a: 1, b: "foo" };
    let x1 = { a: 1, b: "foo" };
    let x2: { a : 0 | 1, b: "foo" | "bar" } = { a: 1, b: "foo" };
}

function f5(): void {
    const c1 = [1, "foo"];
    const c2: (1 | "foo")[] = [1, "foo"];
    const c3: [1, "foo"] = [1, "foo"];
    let x1 = [1, "foo"];
    let x2: (1 | "foo")[] = [1, "foo"];
    let x3: [1, "foo"] = [1, "foo"];
}

/*pruned*/;          
                                                                              
                                                                            
 

function f10(): string {
    return "hello";
}

function f11(): 1 | "two" {
    return cond ? 1 : "two";
}

function f12(): 1 | "two" {
    if (cond) {
        return 1;
    }
    else {
        return "two";
    }
}

class C2 {
    foo(): number {
        return 0;
    }
    bar(): 1 | 0 {
        return cond ? 0 : 1;
    }
}

function f20(): void {
    const f1 = () => 0;
    const f2 = () => "hello";
    const f3 = () => true;
    const f4 = () => E.C;
    const f5 = (): "foo" => "foo";
    const f6: () => "foo" | "bar" = () => "bar";
    const f7: (() => "foo") | (() => "bar") = () => "bar";
}

/*pruned*/;                                               
/*pruned*/;                                                     
/*pruned*/;                                                                
/*pruned*/;                                                   
/*pruned*/;                                                                        
/*pruned*/;                                                 
/*pruned*/;                                                     
/*pruned*/;                                                               

const a: (1 | 2)[] = [1, 2];

/*pruned*/;        // Type 1
/*pruned*/;           // Type 1
/*pruned*/;           // Type 1 | 2
/*pruned*/;               // Type 1 | "two"
/*pruned*/;        // Type number[]
/*pruned*/;           // Type (1 | 2)[]
/*pruned*/;             // Type number
/*pruned*/;        // Type 1 | 2
/*pruned*/;        // Type (1 | 2)[]
/*pruned*/;                 // Type number
/*pruned*/;                     // Type number

function makeArray<T>(x: T): T[] {
    return [x];
}

function append<T>(a: T[], x: T): T[] {
    let result = a.slice();
    result.push(x);
    return result;
}

type Bit = 0 | 1;

let aa = makeArray<Bit>(0);
aa = append(aa, 1);


function main(): void {}
