// @target:es6
// ElementList:  ( Modified )
//      Elisionopt   AssignmentExpression
//      Elisionopt   SpreadElement
//      ElementList, Elisionopt   AssignmentExpression
//      ElementList, Elisionopt   SpreadElement

// SpreadElement:
//      ...   AssignmentExpression

/*pruned*/;           
let a1 = ["hello", "world"]
/*pruned*/;                     
/*pruned*/;         
let a4 = [() => 1, ];
/*pruned*/;         

// Each element expression in a non-empty array literal is processed as follows:
//    - If the array literal contains no spread elements, and if the array literal is contextually typed (section 4.19)
//      by a type T and T has a property with the numeric name N, where N is the index of the element expression in the array literal,
//      the element expression is contextually typed by the type of that property.

// The resulting type an array literal expression is determined as follows:
//     - If the array literal contains no spread elements and is contextually typed by a tuple-like type,
//       the resulting type is a tuple type constructed from the types of the element expressions.

/*pruned*/;                                  
let b1: [number[], string[]] = [[1, 2, 3], ["hello", "string"]];

// The resulting type an array literal expression is determined as follows:
//     - If the array literal contains no spread elements and is an array assignment pattern in a destructuring assignment (section 4.17.1),
//       the resulting type is a tuple type constructed from the types of the element expressions.

let [c0, c1] = [1, 2];        // tuple type [number, number]
let [c2, c3] = [1, 2, true];  // tuple type [number, number, boolean]

// The resulting type an array literal expression is determined as follows:
//      - the resulting type is an array type with an element type that is the union of the types of the
//        non - spread element expressions and the numeric index signature types of the spread element expressions
let temp = ["s", "t", "r"];
let temp1 = [1, 2, 3];
let temp2: [number[], string[]] = [[1, 2, 3], ["hello", "string"]];

/*pruned*/;                                
/*pruned*/;                                        
let d0 = [1, true, ...temp, ];  // has type (string|number|boolean)[]
let d1 = [...temp];            // has type string[]
let d2: number[] = [...temp1];
/*pruned*/;                  
/*pruned*/;                            
/*pruned*/;      
/*pruned*/;      
let d7 = [...a4];
let d8: number[][] = [[...temp1]]
let d9 = [[...temp1], ...["hello"]];

function main(): void {}
