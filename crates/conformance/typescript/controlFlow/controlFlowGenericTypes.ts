// @target: es2015
// @strict: true

function f1<T extends string | undefined>(x: T, y: { a: T }, z: [T]): string {
    if (x) {
        x;
        x.length;
        return x;
    }
    if (y.a) {
        y.a.length;
        return y.a;
    }
    if (z[0]) {
        z[0].length;
        return z[0];
    }
    return "hello";
}

function f2<T>(x: Extract<T, string | undefined> | null): string {
    if (x) {
        x;
        x.length;
        return x;
    }
    return "hello";
}

interface Box<T> {
    item: T;
}

/*pruned*/;                                                                       
function isUndefined(x: unknown): x is undefined { return null as unknown as (boolean); }
/*pruned*/;                                                       

function g1<T extends Box<T> | undefined>(x: T): void {
    /*pruned*/;    
                 
     
}

function g2<T extends Box<T> | undefined>(x: T): void {
    if (!isUndefined(x)) {
        /**/;    
    }
}

function g3<T extends Box<T> | undefined>(x: T): void {
    /*pruned*/;     
                           
     
}

function g4<T extends Box<T> | undefined>(x: T): void {
    if (isUndefined(x)) {
        /**/;      // Error
    }
}

// Repro from #13995

function takeA(val: 'A'): void { }
export function bounceAndTakeIfA<AB extends 'A' | 'B'>(value: AB): AB {
    if (value === 'A') {
        takeA(value);
        return value;
    }
    else {
        return value;
    }
}

// Repro from #13995

type Common = { id: number };
type AA = { tag: 'A', id: number };
type BB = { tag: 'B', id: number, foo: number };

type MyUnion = AA | BB;

const fn = (value: MyUnion) => {
    value.foo;  // Error
    if ('foo' in value) {
        value.foo;
    }
    if (value.tag === 'B') {
        value.foo;
    }
};

/*pruned*/;                                            
                        
                         
                  
     
                            
                  
     
  

// Repro from #13995

type A1 = {
    testable: true
    doTest: () => void
}
type B1 = {
    testable: false
};

type Union = A1 | B1

function notWorking<T extends Union>(object: T): void {
    if (!object.testable) return;
    object.doTest();
}

// Repro from #42939

interface A {
    a: number | null;
};

function get<K extends keyof A>(key: K, obj: A): number {
    const value = obj[key];
    if (value !== null) {
        return value;
    }
    return 0;
};

// Repro from #44093

/*pruned*/;             
                                                                             
 
/*pruned*/;                                                                                  
                                     
                                                         
 

// In an element access obj[x], we consider obj to be in a constraint position, except when obj is of
// a generic type without a nullable constraint and x is a generic type. This is because when both obj
// and x are of generic types T and K, we want the resulting type to be T[K].

function fx1<T, K extends keyof T>(obj: T, key: K): void {
    const x1 = obj[key];
    const x2 = obj && obj[key];
}

function fx2<T extends Record<keyof T, string>, K extends keyof T>(obj: T, key: K): void {
    const x1 = obj[key];
    const x2 = obj && obj[key];
}

function fx3<T extends Record<keyof T, string> | undefined, K extends keyof T>(obj: T, key: K): void {
    const x1 = obj[key];  // Error
    const x2 = obj && obj[key];
}

// Repro from #44166

/*pruned*/;         
                                                       
                                                                                  
               
                                          
                                                                                
                                                                                
                                  
                   
         
                                           
                                         
     
 

// Repros from #45145

function f10<T extends { a: string } | undefined>(x: T, y: Partial<T>): void {
    y = x;
}

/*pruned*/;                                                                       

/*pruned*/;        
                                                                 
     
                                                  
                              
     
 

// Repro from #46495

interface Button {
    type: "button";
    text: string;
}

interface Checkbox {
    type: "checkbox";
    isChecked: boolean;
}

type Control = Button | Checkbox;

function update<T extends Control, K extends keyof T>(control : T | undefined, key: K, value: T[K]): void {
    if (control !== undefined) {
        control[key] = value;
    }
}

// Repro from #50465

/*pruned*/;                                                                                          

/*pruned*/;                                                                                                                                                          
                     
 


function main(): void {}
