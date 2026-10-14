// @target: es2015
// typeof  operator on string type
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
let ResultIsString1 = typeof STRING;
let ResultIsString2 = typeof STRING1;

// string type literal
let ResultIsString3 = typeof "";
let ResultIsString4 = typeof { x: "", y: "" };
let ResultIsString5 = typeof { x: "", y: (s: string) => { return s; } };

// string type expressions
let ResultIsString6 = typeof objA.a;
/*pruned*/;                      
let ResultIsString8 = typeof STRING1[0];
let ResultIsString9 = typeof foo();
let ResultIsString10 = typeof A.foo();
let ResultIsString11 = typeof (STRING + STRING);
let ResultIsString12 = typeof STRING.charAt(0);

// multiple typeof  operators
let ResultIsString13 = typeof typeof STRING;
let ResultIsString14 = typeof typeof typeof (STRING + STRING);

// miss assignment operators
typeof "";
typeof STRING;
typeof STRING1;
typeof foo();
/*pruned*/;        

// use typeof in type query
let z: string = null as unknown as (string);
let x: string[] = null as unknown as (string[]);
let r: () => string = null as unknown as (() => string);
/*pruned*/;      
/*pruned*/;       
/*pruned*/;   
let y = { a: "", b: "" };
/*pruned*/;   
/*pruned*/;      
/*pruned*/;     
/*pruned*/;   

function main(): void {}
