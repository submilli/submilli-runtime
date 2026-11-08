// @target: es6

// If the parameter is a rest parameter, the parameter type is any[]
// A type annotation for a rest parameter must denote an array type.

// RestParameter:
//     ...   Identifier   TypeAnnotation(opt)

type arrayString = Array<String>
type someArray = Array<String> | number[];
type stringOrNumArray = Array<String|Number>;

function a1(...x: (number|string)[]): void { }
/*pruned*/;                
function a3(...a: Array<String>): void { }
function a4(...a: arrayString): void { }
function a5(...a: stringOrNumArray): void { }
/*pruned*/;                         
/*pruned*/;                                
function a11([a, b, c, ...x]: number[]): void { }


let array = [1, 2, 3];
let array2 = [true, false, "hello"];
/*pruned*/;    
a1(...array);

/*pruned*/;                              // Parameter type is [any, any, [[any]]]

/*pruned*/;                               // Parameter type is any[]
/*pruned*/;                               // Parameter type is any[]
/*pruned*/;                               // Parameter type is any[]
a11([1, 2]);                              // Parameter type is number[]

// Rest parameter with generic
function foo<T>(...a: T[]): void { }
foo<number|string>("hello", 1, 2);
foo("hello", "world");

enum E { a, b }
/*pruned*/;           
function foo1<T extends Number>(...a: T[]): void { }
foo1(1, 2, 3, E.a);
/*pruned*/;              




function main(): void {}
