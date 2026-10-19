// @target: es2015
// @strict: true
// void  operator on number type
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
let ResultIsAny1 = void NUMBER;
let ResultIsAny2 = void NUMBER1;

// number type literal
let ResultIsAny3 = void 1;
let ResultIsAny4 = void { x: 1, y: 2};
let ResultIsAny5 = void { x: 1, y: (n: number) => { return n; } };

// number type expressions
let ResultIsAny6 = void objA.a;
/*pruned*/;                 
let ResultIsAny8 = void NUMBER1[0];
let ResultIsAny9 = void foo();
let ResultIsAny10 = void A.foo();
let ResultIsAny11 = void (NUMBER + NUMBER);

// multiple void  operators
let ResultIsAny12 = void void NUMBER;
let ResultIsAny13 = void void void (NUMBER + NUMBER);

// miss assignment operators
void 1;
void NUMBER;
void NUMBER1;
void foo();
void objA.a;
/**/;    
/*pruned*/;      

function main(): void {}
