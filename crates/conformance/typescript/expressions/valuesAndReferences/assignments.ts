// @target: es2015
// @strict: true
// In this file:
//  Assign to a module
//  Assign to a class
//  Assign to an enum
//  Assign to a function
//  Assign to a variable
//  Assign to a parameter
//  Assign to an interface

/*pruned*/;    
/**/;     // Error

class C { }
C = null; // Error

enum E { A }
E = null; // Error
E.A = null; // OK per spec, Error per implementation (509581)

function fn(): void { }
fn = null; // Should be error

/**/; 
/**/;     // OK

/*pruned*/;            
                   
 

interface I { }
I = null; // Error

function main(): void {}
