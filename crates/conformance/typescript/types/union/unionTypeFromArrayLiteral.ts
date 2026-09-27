// @target: es2015
// The resulting type an array literal expression is determined as follows:
// If the array literal is empty, the resulting type is an array type with the element type Undefined.
// Otherwise, if the array literal is contextually typed by a type that has a property with the numeric name ‘0’, the resulting type is a tuple type constructed from the types of the element expressions.
// Otherwise, the resulting type is an array type with an element type that is the union of the types of the element expressions.

let arr1 = [1, 2]; // number[]
let arr2 = ["hello", true]; // (string | number)[]
let arr3Tuple: [number, string] = [3, "three"]; // [number, string]
let arr4Tuple: [number, string] = [3, "three", "hello"]; // [number, string, string]
let arrEmpty = [];
/*pruned*/;     
              
              
                                                       // Tuple
class C { foo(): void { } }
class D { foo2(): void { } }
class E extends C { foo3(): void { } }
class F extends C { foo4(): void { } }
/*pruned*/;                                                                                                                    
/*pruned*/;         // (C | D)[]
/*pruned*/;           // (C | D)[]
/*pruned*/;        // C[]
/*pruned*/;        // (E|F)[]

function main(): void {}
