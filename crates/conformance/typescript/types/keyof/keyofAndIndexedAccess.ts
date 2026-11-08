// @target: es2015
// @strictNullChecks: true
// @strictPropertyInitialization: false
// @declaration: true

class Shape {
    name: string;
    width: number;
    height: number;
    visible: boolean;
}

class TaggedShape extends Shape {
    tag: string;
}

class Item {
    name: string;
    price: number;
}

class Options {
    visible: "yes" | "no";
}

type Dictionary<T> = { [x: string]: T };
type NumericallyIndexed<T> = { [x: number]: T };

/*pruned*/;             

/*pruned*/;            // string
type K01 = keyof string;  // "toString" | "charAt" | ...
type K02 = keyof number;  // "toString" | "toFixed" | "toExponential" | ...
type K03 = keyof boolean;  // "valueOf"
type K04 = keyof void;  // never
type K05 = keyof undefined;  // never
type K06 = keyof null;  // never
type K07 = keyof never;  // string | number | symbol
type K08 = keyof unknown; // never

type K10 = keyof Shape;  // "name" | "width" | "height" | "visible"
type K11 = keyof Shape[];  // "length" | "toString" | ...
type K12 = keyof Dictionary<Shape>;  // string
type K13 = keyof {};  // never
type K14 = keyof Object;  // "constructor" | "toString" | ...
/*pruned*/;          // "toString" | "toFixed" | "toExponential" | ...
type K16 = keyof [string, number];  // "0" | "1" | "length" | "toString" | ...
type K17 = keyof (Shape | Item);  // "name"
/*pruned*/;                       // "name" | "width" | "height" | "visible" | "price"
type K19 = keyof NumericallyIndexed<Shape> // never

type KeyOf<T> = keyof T;

type K20 = KeyOf<Shape>;  // "name" | "width" | "height" | "visible"
type K21 = KeyOf<Dictionary<Shape>>;  // string

type NAME = "name";
type WIDTH_OR_HEIGHT = "width" | "height";

type Q10 = Shape["name"];  // string
type Q11 = Shape["width" | "height"];  // number
type Q12 = Shape["name" | "visible"];  // string | boolean

type Q20 = Shape[NAME];  // string
type Q21 = Shape[WIDTH_OR_HEIGHT];  // number

type Q30 = [string, number][0];  // string
type Q31 = [string, number][1];  // number
type Q32 = [string, number][number];  // string | number
/*pruned*/;                        // string
/*pruned*/;                        // number
type Q35 = [string, number]["0"];  // string
type Q36 = [string, number]["1"];  // string

type Q40 = (Shape | Options)["visible"];  // boolean | "yes" | "no"
/*pruned*/;                               // true & "yes" | true & "no" | false & "yes" | false & "no"

type Q50 = Dictionary<Shape>["howdy"];  // Shape
type Q51 = Dictionary<Shape>[123];  // Shape
/*pruned*/;                         // Shape

let cond: boolean = null as unknown as (boolean);

function getProperty<T, K extends keyof T>(obj: T, key: K): T[K] {
    return obj[key];
}

function setProperty<T, K extends keyof T>(obj: T, key: K, value: T[K]): void {
    obj[key] = value;
}

function f10(shape: Shape): void {
    let name = getProperty(shape, "name");  // string
    let widthOrHeight = getProperty(shape, cond ? "width" : "height");  // number
    let nameOrVisible = getProperty(shape, cond ? "name" : "visible");  // string | boolean
    setProperty(shape, "name", "rectangle");
    setProperty(shape, cond ? "width" : "height", 10);
    setProperty(shape, cond ? "name" : "visible", true);  // Technically not safe
}

function f11(a: Shape[]): void {
    let len = getProperty(a, "length");  // number
    setProperty(a, "length", len);
}

function f12(t: [Shape, boolean]): void {
    let len = getProperty(t, "length");
    let s2 = getProperty(t, "0");  // Shape
    let b2 = getProperty(t, "1");  // boolean
}

/*pruned*/;                             
                                          
                                            
                                          
 

/*pruned*/;                
                    
                                                                
                               
     
                                                                             
                                
     
 

/*pruned*/;                                      
                                                        
                                                                                    
                                                                                              
                                               
                                                        
                                                                                    
 

function pluck<T, K extends keyof T>(array: T[], key: K): T[K][] {
    return array.map(x => x[key]);
}

function f30(shapes: Shape[]): void {
    let names = pluck(shapes, "name");    // string[]
    let widths = pluck(shapes, "width");  // number[]
    let nameOrVisibles = pluck(shapes, cond ? "name" : "visible");  // (string | boolean)[]
}

function f31<K extends keyof Shape>(key: K): Shape[K] {
    const shape: Shape = { name: "foo", width: 5, height: 10, visible: true };
    return shape[key];  // Shape[K]
}

function f32<K extends "width" | "height">(key: K): Shape[K] {
    const shape: Shape = { name: "foo", width: 5, height: 10, visible: true };
    return shape[key];  // Shape[K]
}

function f33<S extends Shape, K extends keyof S>(shape: S, key: K): S[K] {
    let name = getProperty(shape, "name");
    let prop = getProperty(shape, key);
    return prop;
}

function f34(ts: TaggedShape): void {
    let tag1 = f33(ts, "tag");
    let tag2 = getProperty(ts, "tag");
}

/**/;    
                     
                        
                      
 

// Indexed access expressions have always permitted access to private and protected members.
// For consistency we also permit such access in indexed access types.
/*pruned*/;               
                    
                    
                    
                      
                      
                      
 

function f50<T>(k: keyof T, s: string): void {
    const x1 = s as keyof T;
    const x2 = k as string;
}

function f51<T, K extends keyof T>(k: K, s: string): void {
    const x1 = s as keyof T;
    const x2 = k as string;
}

/*pruned*/;                                                                                              
                      
                      
                      
 

/*pruned*/;                                                                                                           
                      
                      
                      
 

function f54<T>(obj: T, key: keyof T): void {
    /*pruned*/;              
     
    const b = "foo" in obj[key];
}

function f55<T, K extends keyof T>(obj: T, key: K): void {
    /*pruned*/;              
     
    const b = "foo" in obj[key];
}

function f60<T>(source: T, target: T): void {
    /*pruned*/;            
                              
     
}

/*pruned*/;                                                                     
                                                           
                                                           
                                                           
 

/*pruned*/;                                                     
                                                    
                               
                               
                                
 

/*pruned*/;                                                                                    
                                                                    
                                                                    
                                                                     
 

/*pruned*/;                                                                                
                                                                    
                                                                    
                                                                     
 

/*pruned*/;                                                                                
                                                                          
                                                                                    
 

/*pruned*/;                                              
                                  
                                     
                                           
                             
                                   
                                                          
 

/*pruned*/;                                                     
                                        
 

function f82(): void {
    /*pruned*/;                           // string
    /*pruned*/;                      // number
}

/*pruned*/;                                                                                        
                                      
 

function f84(): void {
    /*pruned*/;                                    // string
    /*pruned*/;                               // number
}

/**/;     
              
                                                
                         
     
                                                             
                          
     
                 
                                   
                                      
                                             
                                                     
                    
                       
                          
                                   
     
 

type S2 = {
    a: string;
    b: string;
};

function f90<T extends S2, K extends keyof S2>(x1: S2[keyof S2], x2: T[keyof S2], x3: S2[K]): void {
    x1 = x2;
    x1 = x3;
    x2 = x1;
    x2 = x3;
    x3 = x1;
    x3 = x2;
    x1.length;
    x2.length;
    x3.length;
}

function f91<T, K extends keyof T>(x: T, y: T[keyof T], z: T[K]): void {
    let a: {} = null as unknown as ({});
    a = x;
    a = y;
    a = z;
}

function f92<T, K extends keyof T>(x: T, y: T[keyof T], z: T[K]): void {
    let a: {} | null | undefined = null as unknown as ({} | null | undefined);
    a = x;
    a = y;
    a = z;
}

// Repros from #12011

/*pruned*/; 
                                                 
                          
     
                                                              
                           
     
 

/*pruned*/;                
                  
                                
                
                                 
     
                               
                                
     
 

/*pruned*/;        
                  
                                
                                          
     
                               
                                         
     
 

// Modified repro from #12544

/*pruned*/;                                                   
/*pruned*/;                                                                                         
/*pruned*/;                                                                                                                                   
/*pruned*/;                                                
/*pruned*/;                                                 
                     
                         
                           
     
                  
 

type Thing = {
    a: { x: number, y: string },
    b: boolean
};


function f1(thing: Thing): void {
    /*pruned*/;                 // { x: number, y: string }
    /*pruned*/;                      // string
    /*pruned*/;                 // boolean
    /*pruned*/;                           // any
}

// Repro from comment in #12114

/*pruned*/;                                                                                        
                                                     

// Modified repro from #12573

/*pruned*/;                                                                   
/*pruned*/;                                          

                                                        
/*pruned*/;                                                                   
/*pruned*/;                                 // {}
/*pruned*/;                                           // { test: boolean }

// Repro from #12624

interface Options1<Data, Computed> {
    data?: Data
    computed?: Computed;
}

/*pruned*/;                               
                                                   
                                                                         
 

/*pruned*/;              
           
                 
     
   

/*pruned*/;     

// Repro from #12625

interface Options2<Data, Computed> {
    data?: Data
    computed?: Computed;
}

/*pruned*/;                               
                                                   
                                                                             
 

// Repro from #12641

interface R {
    p: number;
}

function f<K extends keyof R>(p: K): void {
    /*pruned*/;                           
    /**/;      // any
}

// Repro from #12651

/*pruned*/;              
              
             
                  
 

/*pruned*/;                                                                                                                                              

type SomeMethodDescriptor = {
	name: "someMethod";
	args: [string, number];
	returnValue: string[];
}

/*pruned*/;                                                                    

// Repro from #13073

type KeyTypes = "a" | "b"
/*pruned*/;                                                                                          

function addToMyThingy<S extends KeyTypes>(key: S): void {
    /*pruned*/;             
}

// Repro from #13102

type Handler<T> = {
    onChange: (name: keyof T) => void;
};

/*pruned*/;                                                                        
                              
 

// Repro from #13285

function updateIds<T extends Record<K, string>, K extends string>(
    obj: T,
    idFields: K[],
    idMapping: Partial<Record<T[K], T[K]>>
): Record<K, string> {
    for (const idField of idFields) {
        const newId: T[K] | undefined = idMapping[obj[idField]];
        if (newId) {
            obj[idField] = newId;
        }
    }
    return obj;
}

// Repro from #13285

function updateIds2<T extends { [x: string]: string }, K extends keyof T>(
    obj: T,
    key: K,
    stringMap: { [oldId: string]: string }
): void {
    let x = obj[key];
    stringMap[x]; // Should be OK.
}

// Repro from #13514

/*pruned*/;                                                                             

// Repro from #13604

/*pruned*/; 
                            
 

/*pruned*/;                      
                            
      
  
 

// Repro from #13749

/*pruned*/;    
                                                                         

                                                               
                                            
     
 

// Repro from #13787

class SampleClass<P> {
    public props: Readonly<P>;
    constructor(props: P) {
        this.props = Object.freeze(props);
    }
}

interface Foo {
    foo: string;
}

/*pruned*/;                                                                         

/*pruned*/;                                               
                           
                                        
                                 
     

                                 
                              
     
 
/*pruned*/;                

// Positive repro from #17166
function f3<T, K extends Extract<keyof T, string>>(t: T, k: K, tk: T[K]): void {
    /*pruned*/;         
                                    
                                               
     
}

// # 21185
/*pruned*/;                      
                                                                                                      
 

// Repros from #23592

/*pruned*/;                                                                                  
/*pruned*/;                                                         

/*pruned*/;                                                        
/*pruned*/;                                                           
/*pruned*/;                                                           

// Repro from #23618

/*pruned*/;                                              
enum Flag {
    FLAG_1 = "flag_1",
    FLAG_2 = "flag_2"
}

/*pruned*/;                                                                           
/*pruned*/;                                                                                                                      
                            
 

/*pruned*/;                                                                                                          
/*pruned*/;                                                                                                                         
                            
 

// Repro from #21368

interface I {
    foo: string;
}

function take<T>(p: T): void { }

function fn<T extends I, K extends keyof T>(o: T, k: K): void {
    take<{} | null | undefined>(o[k]);
    /*pruned*/;     
}

// Repro from #23133

class Unbounded<T> {
    foo(x: T[keyof T]): void {
        let y: {} | undefined | null = x;
    }
}

// Repro from #23940

/*pruned*/;   
           
 
type Foo7<T extends number> = T;
/*pruned*/;                                                                                       

// Repro from #21770

/*pruned*/;                                          
/*pruned*/;                                                                 

/*pruned*/;                                                                                 
                      
 

/*pruned*/;                                                                                 
                              
                 
 

// Repro from #26409

/*pruned*/;                                                                                        
 
                           
           
  

/*pruned*/;                                                                             
 
                           
           
  


function main(): void {}
