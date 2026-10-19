// @target: es2015
// @noImplicitAny: false
// @allowUnusedLabels: true

// typeof  operator on boolean type
let BOOLEAN: boolean = null as unknown as (boolean);

function foo(): boolean { return true; }

class A {
    public a: boolean;
    static foo(): boolean { return false; }
}
/*pruned*/;  
                                                   
 

let objA = new A();

// boolean type var
let ResultIsString1 = typeof BOOLEAN;

// boolean type literal
let ResultIsString2 = typeof true;
let ResultIsString3 = typeof { x: true, y: false };

// boolean type expressions
let ResultIsString4 = typeof objA.a;
/*pruned*/;                      
let ResultIsString6 = typeof foo();
let ResultIsString7 = typeof A.foo();

// multiple typeof  operator
let ResultIsString8 = typeof typeof BOOLEAN;

// miss assignment operators
typeof true;
typeof BOOLEAN;
typeof foo();
/*pruned*/;        
typeof objA.a;
/*pruned*/;

// use typeof in type query
let z: boolean = null as unknown as (boolean);
let x: boolean[] = null as unknown as (boolean[]);
let r: () => boolean = null as unknown as (() => boolean);
/*pruned*/;       
/*pruned*/;   
let y = { a: true, b: false};
/*pruned*/;   
/*pruned*/;      
/*pruned*/;     
/*pruned*/;   

function main(): void {}
