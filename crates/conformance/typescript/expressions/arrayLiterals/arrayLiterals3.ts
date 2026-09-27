// @target: es2015
// Each element expression in a non-empty array literal is processed as follows:
//    - If the array literal contains no spread elements, and if the array literal is contextually typed (section 4.19)
//      by a type T and T has a property with the numeric name N, where N is the index of the element expression in the array literal,
//      the element expression is contextually typed by the type of that property.

// The resulting type an array literal expression is determined as follows:
//     - If the array literal contains no spread elements and is contextually typed by a tuple-like type,
//       the resulting type is a tuple type constructed from the types of the element expressions.

/*pruned*/;                                               // Error
let a1: [boolean, string, number] = ["string", 1, true];  // Error

// The resulting type an array literal expression is determined as follows:
//     - If the array literal contains no spread elements and is an array assignment pattern in a destructuring assignment (section 4.17.1),
//       the resulting type is a tuple type constructed from the types of the element expressions.

let [b1, b2]: [number, number] = [1, 2, "string", true];

// The resulting type an array literal expression is determined as follows:
//      - the resulting type is an array type with an element type that is the union of the types of the
//        non - spread element expressions and the numeric index signature types of the spread element expressions
let temp = ["s", "t", "r"];
let temp1 = [1, 2, 3];
let temp2: [number[], string[]] = [[1, 2, 3], ["hello", "string"]];

/*pruned*/;    
                         
                         
 
/*pruned*/;                                
/*pruned*/;                                        
/*pruned*/;                                       // Error
let c1: [number, number, number] = [...temp1];    // Error cannot assign number[] to [number, number, number]
/*pruned*/;                                       // Error cannot assign (number|string)[] to number[]


function main(): void {}
