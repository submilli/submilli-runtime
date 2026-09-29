// @target: es2015
// If the parameter is a rest parameter, the parameter type is any[]
// A type annotation for a rest parameter must denote an array type.

// RestParameter:
//     ...   Identifier   TypeAnnotation(opt)

type arrayString = Array<String>
/*pruned*/;                               
type stringOrNumArray = Array<String|Number>;

function a0(...x: [number, number, string]): void { }  // Error, rest parameter must be array type
function a1(...x: (number|string)[]): void { }
/*pruned*/;                             // Error, rest parameter must be array type
/*pruned*/;                             // Error, can't be optional
function a4(...b: number[] = [1,2,3]): void { }   // Error, can't have initializer
/*pruned*/;                         
function a6([a, b, c, ...x]: number[]): void { }


a1(1, 2, "hello", true);  // Error, parameter type is (number|string)[]
/*pruned*/;               // Error parameter type is (number|string)[]
/*pruned*/;                              // Error, parameter type is [any, any, [[any]]]
/*pruned*/;                              // Error, parameter type is [any, any, [[any]]]
a6([1, 2, "string"]);                   // Error, parameter type is number[]


let temp = [1, 2, 3];
/**/;    
                                                                                   
 

// Rest parameter with generic
/*pruned*/;                                         
/*pruned*/;                       // Error




function main(): void {}
