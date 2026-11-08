// @target: es2015
class Shape {
    name: string;
    width: number;
    height: number;
    visible: boolean;
}

type Dictionary<T> = { [x: string]: T };

type T00 = keyof K0;  // Error

type T01 = keyof Object;
type T02 = keyof keyof Object;
type T03 = keyof keyof keyof Object;
type T04 = keyof keyof keyof keyof Object;
type T05 = keyof keyof keyof keyof keyof Object;
type T06 = keyof keyof keyof keyof keyof keyof Object;

type T10 = Shape["name"];
type T11 = Shape["foo"];  // Error
type T12 = Shape["name" | "foo"];  // Error
/*pruned*/;             // Error
type T14 = Shape[string];  // Error
type T15 = Shape[number];  // Error
type T16 = Shape[boolean];  // Error
type T17 = Shape[void];  // Error
type T18 = Shape[undefined];  // Error
type T19 = Shape[{ x: string }];  // Error
type T20 = Shape[string | number];  // Error
/*pruned*/;                       
type T22 = Shape[string | boolean];  // Error

type T30 = string[]["length"];
type T31 = string[][number];
type T32 = string[][string];  // Error
type T33 = string[][boolean];  // Error

/*pruned*/;                        
type T41 = Dictionary<string>[number];
type T42 = Dictionary<string>[string];
type T43 = Dictionary<string>[boolean];  // Error

/*pruned*/;         
/*pruned*/;            
/*pruned*/;            
/*pruned*/;               // Error

type T60 = {}["toString"];
/*pruned*/;               

let cond: boolean = null as unknown as (boolean);

function getProperty<T, K extends keyof T>(obj: T, key: K): T[K] {
    return obj[key];
}

function setProperty<T, K extends keyof T>(obj: T, key: K, value: T[K]): void {
    obj[key] = value;
}

function f10(shape: Shape): void {
    let x1 = getProperty(shape, "name");
    let x2 = getProperty(shape, "size");  // Error
    let x3 = getProperty(shape, cond ? "name" : "size");  // Error
    setProperty(shape, "name", "rectangle");
    setProperty(shape, "size", 10);  // Error
    setProperty(shape, cond ? "name" : "size", 10);  // Error
}

/*pruned*/;                                                                                                                       
          
          
                    
                    

          
          
          
          

            
                      
                      

            
                      
                      

            
            
            

            
            
            
 

// Repro from #17166
function f3<T, K extends Extract<keyof T, string>, U extends T, J extends K>(
    t: T, k: K, tk: T[K], u: U, j: J, uk: U[K], tj: T[J], uj: U[J]): void {
    /*pruned*/;         
                                    
                                        
                                               
                                                   
     
    tk = uk;
    uk = tk; // error

    tj = uj;
    uj = tj; // error

    tk = tj;
    tj = tk; // error

    tk = uj;
    uj = tk; // error
}

// The constraint of 'keyof T' is 'keyof T'
/*pruned*/;                                                          
                    
                         
 

// Repro from #27470

/*pruned*/;                                          
                                                    
  

type MyType = {a: string, b: string | undefined}

/*pruned*/;                          

/*pruned*/;                    // Error
/*pruned*/;                  

/*pruned*/;                                                                         
                        
                             
                              
 

// Repro from #28839

function f30<T, K extends keyof T>(): void {
    let x: Partial<Record<keyof T, string>>[K] = "hello";
}

function f31<T, K extends keyof T>(): void {
    let x: Partial<Partial<Partial<Partial<Partial<Partial<Partial<Record<keyof T, string>>>>>>>>[K] = "hello";
}

// Repro from #51069

class Test<T extends Record<string, number>> {
  testy: T;

  constructor(t: T) {
    this.testy = t;
  }

  public t(key: keyof T): number {
    this.testy[key] += 1; // Error

    return this.testy[key];
  }
}


function main(): void {}
