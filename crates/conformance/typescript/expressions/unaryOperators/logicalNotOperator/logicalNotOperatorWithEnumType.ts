// @target: es2015
// ! operator on enum type

enum ENUM { A, B, C };
enum ENUM1 { };

// enum type var
/*pruned*/;                  

// enum type expressions
let ResultIsBoolean2 = !ENUM["B"];
let ResultIsBoolean3 = !(ENUM.B + ENUM["C"]);

// multiple ! operators
/*pruned*/;                   
let ResultIsBoolean5 = !!!(ENUM["B"] + ENUM.C);

// miss assignment operators
/**/; 
/**/;  
!ENUM.B;
/*pruned*/;  

function main(): void {}
