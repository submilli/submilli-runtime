// @target: es2015
// In a contextually typed array literal expression containing no spread elements, an element expression at index N is contextually typed by
//      the type of the property with the numeric name N in the contextual type, if any, or otherwise
//      the numeric index type of the contextual type, if any.
let array = [1, 2, 3];
let array1 = [true, 2, 3];  // Contextual type by the numeric index type of the contextual type
let tup: [number, number, number] = [1, 2, 3, 4];
let tup1: [number|string, number|string, number|string] = [1, 2, 3, "string"];
let tup2: [number, number, number] = [1, 2, 3, "string"];  // Error

// In a contextually typed array literal expression containing one or more spread elements,
// an element expression at index N is contextually typed by the numeric index type of the contextual type, if any.
let spr = [1, 2, 3, ...array];
let spr1 = [1, 2, 3, ...tup];
let spr2:[number, number, number] = [1, 2, 3, ...tup];  // Error


function main(): void {}
