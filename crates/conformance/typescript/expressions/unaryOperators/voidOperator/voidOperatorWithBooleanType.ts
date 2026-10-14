// @target: es2015
// @strict: true
// void  operator on boolean type
let BOOLEAN: boolean = null as unknown as (boolean);

function foo(): boolean { return true; }

class A {
    public a: boolean;
    static foo(): boolean { return false; }
}
/*pruned*/;  
                                                  
 

let objA = new A();

// boolean type var
let ResultIsAny1 = void BOOLEAN;

// boolean type literal
let ResultIsAny2 = void true;
let ResultIsAny3 = void { x: true, y: false };

// boolean type expressions
let ResultIsAny4 = void objA.a;
/*pruned*/;                 
let ResultIsAny6 = void foo();
let ResultIsAny7 = void A.foo();

// multiple void  operator
let ResultIsAny8 = void void BOOLEAN;

// miss assignment operators
void true;
void BOOLEAN;
void foo();
/*pruned*/;      
void objA.a;
/**/;    

function main(): void {}
