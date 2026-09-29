// @target: es2015
// The type T associated with a destructuring variable declaration is determined as follows:
//      If the declaration includes a type annotation, T is that type.
let {a1, a2}: { a1: number, a2: string } = { a1: 10, a2: "world" }
/*pruned*/;                                                                  

// The type T associated with a destructuring variable declaration is determined as follows:
//      Otherwise, if the declaration includes an initializer expression, T is the type of that initializer expression.
/*pruned*/;                                                         
let temp = { t1: true, t2: "false" };
/*pruned*/;                                                                 
/*pruned*/;                                             

// The type T associated with a binding element is determined as follows:
//      If the binding element is a rest element, T is an array type with
//          an element type E, where E is the type of the numeric index signature of S.
let [...c1] = [1,2,3]; 
let [...c2] = [1,2,3, "string"]; 

// The type T associated with a binding element is determined as follows:
//      Otherwise, if S is a tuple- like type (section 3.3.3):
//          	Let N be the zero-based index of the binding element in the array binding pattern.
// 	            If S has a property with the numerical name N, T is the type of that property.
let [d1,d2] = [1,"string"]	

// The type T associated with a binding element is determined as follows:
//      Otherwise, if S is a tuple- like type (section 3.3.3):
//              Otherwise, if S has a numeric index signature, T is the type of the numeric index signature.
let temp1 = [true, false, true]
let [d3, d4] = [1, "string", ...temp1];

//  Combining both forms of destructuring,
/*pruned*/;                                                                      
/*pruned*/;                                                             

// When a destructuring variable declaration, binding property, or binding element specifies
// an initializer expression, the type of the initializer expression is required to be assignable
// to the widened form of the type associated with the destructuring variable declaration, binding property, or binding element.
/*pruned*/;                                                                
/*pruned*/;                                                                   



function main(): void {}
