// @target: es2015
// @allowUnusedLabels: true
// typeof  operator on number type
let NUMBER: number = null as unknown as (number);
let NUMBER1: number[] = [1, 2];

function foo(): number { return 1; }

class A {
    public a: number;
    static foo(): number { return 1; }
}
/*pruned*/;  
                                                 
 

let objA = new A();

// number type var
let ResultIsString1 = typeof NUMBER;
let ResultIsString2 = typeof NUMBER1;

// number type literal
let ResultIsString3 = typeof 1;
let ResultIsString4 = typeof { x: 1, y: 2};
let ResultIsString5 = typeof { x: 1, y: (n: number) => { return n; } };

// number type expressions
let ResultIsString6 = typeof objA.a;
/*pruned*/;                      
let ResultIsString8 = typeof NUMBER1[0];
let ResultIsString9 = typeof foo();
let ResultIsString10 = typeof A.foo();
let ResultIsString11 = typeof (NUMBER + NUMBER);

// multiple typeof  operators
let ResultIsString12 = typeof typeof NUMBER;
let ResultIsString13 = typeof typeof typeof (NUMBER + NUMBER);

// miss assignment operators
typeof 1;
typeof NUMBER;
typeof NUMBER1;
typeof foo();
typeof objA.a;
/*pruned*/;
/*pruned*/;        

// use typeof in type query
let z: number = null as unknown as (number);
let x: number[] = null as unknown as (number[]);
let r: () => number = null as unknown as (() => number);
/*pruned*/;      
/*pruned*/;       
/*pruned*/;   
let y = { a: 1, b: 2 };
/*pruned*/;   
/*pruned*/;      
/*pruned*/;     
/*pruned*/;   

function main(): void {}
