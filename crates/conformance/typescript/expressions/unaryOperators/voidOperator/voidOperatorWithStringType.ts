// @target: es2015
// @strict: true
// void  operator on string type
let STRING: string = null as unknown as (string);
let STRING1: string[] = ["", "abc"];

function foo(): string { return "abc"; }

class A {
    public a: string;
    static foo(): string { return ""; }
}
/*pruned*/;  
                                                
 

let objA = new A();

// string type var
let ResultIsAny1 = void STRING;
let ResultIsAny2 = void STRING1;

// string type literal
let ResultIsAny3 = void "";
let ResultIsAny4 = void { x: "", y: "" };
let ResultIsAny5 = void { x: "", y: (s: string) => { return s; } };

// string type expressions
let ResultIsAny6 = void objA.a;
/*pruned*/;                 
let ResultIsAny8 = void STRING1[0];
let ResultIsAny9 = void foo();
let ResultIsAny10 = void A.foo();
let ResultIsAny11 = void (STRING + STRING);
let ResultIsAny12 = void STRING.charAt(0);

// multiple void  operators
let ResultIsAny13 = void void STRING;
let ResultIsAny14 = void void void (STRING + STRING);

// miss assignment operators
void "";
void STRING;
void STRING1;
void foo();
/*pruned*/;     

function main(): void {}
