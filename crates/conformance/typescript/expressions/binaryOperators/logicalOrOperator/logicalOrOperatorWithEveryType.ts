// @target: es2015
// @strict: true
// The || operator permits the operands to be of any type.
// If the || expression is not contextually typed, the right operand is contextually typed
// by the type of the left operand and the result is of the best common type of the two
// operand types.

enum E { a, b, c }

/*pruned*/;                            
let a2: boolean = null as unknown as (boolean);
let a3: number = null as unknown as (number);
let a4: string = null as unknown as (string);
/*pruned*/;                              
/*pruned*/;                        
let a7: {a: string} = null as unknown as ({a: string});
let a8: string[] = null as unknown as (string[]);

/*pruned*/;                 // any       || any is any
/*pruned*/;                 // boolean   || any is any
/*pruned*/;                 // number    || any is any
/*pruned*/;                 // string    || any is any
/*pruned*/;                 // void      || any is any
/*pruned*/;                 // enum      || any is any
/*pruned*/;                 // object    || any is any
/*pruned*/;                 // array     || any is any
/*pruned*/;                 // null      || any is any
/*pruned*/;            // undefined || any is any

/*pruned*/;                 // any       || boolean is any
let rb2 = a2 || a2;         // boolean   || boolean is boolean
let rb3 = a3 || a2;         // number    || boolean is number | boolean
let rb4 = a4 || a2;         // string    || boolean is string | boolean
/*pruned*/;                 // void      || boolean is void | boolean
/*pruned*/;                 // enum      || boolean is E | boolean
let rb7 = a7 || a2;         // object    || boolean is object | boolean
let rb8 = a8 || a2;         // array     || boolean is array | boolean
let rb9 = null || a2;       // null      || boolean is boolean
let rb10= null || a2;  // undefined || boolean is boolean

/*pruned*/;                 // any       || number is any
let rc2 = a2 || a3;         // boolean   || number is boolean | number
let rc3 = a3 || a3;         // number    || number is number
let rc4 = a4 || a3;         // string    || number is string | number
/*pruned*/;                 // void      || number is void | number
/*pruned*/;                 // enum      || number is number
let rc7 = a7 || a3;         // object    || number is object | number
let rc8 = a8 || a3;         // array     || number is array | number
let rc9 = null || a3;       // null      || number is number
let rc10 = null || a3; // undefined || number is number

/*pruned*/;                 // any       || string is any
let rd2 = a2 || a4;         // boolean   || string is boolean | string
let rd3 = a3 || a4;         // number    || string is number | string
let rd4 = a4 || a4;         // string    || string is string
/*pruned*/;                 // void      || string is void | string
/*pruned*/;                 // enum      || string is enum | string
let rd7 = a7 || a4;         // object    || string is object | string
let rd8 = a8 || a4;         // array     || string is array | string
let rd9 = null || a4;       // null      || string is string
let rd10 = null || a4; // undefined || string is string

/*pruned*/;                 // any       || void is any
/*pruned*/;                 // boolean   || void is boolean | void
/*pruned*/;                 // number    || void is number | void
/*pruned*/;                 // string    || void is string | void
/*pruned*/;                 // void      || void is void
/*pruned*/;                 // enum      || void is enum | void
/*pruned*/;                 // object    || void is object | void
/*pruned*/;                 // array     || void is array | void
/*pruned*/;                 // null      || void is void
/*pruned*/;            // undefined || void is void

/*pruned*/;                 // any       || enum is any
/*pruned*/;                 // boolean   || enum is boolean | enum
/*pruned*/;                 // number    || enum is number
/*pruned*/;                 // string    || enum is string | enum
/*pruned*/;                 // void      || enum is void | enum
/*pruned*/;                 // enum      || enum is E
/*pruned*/;                 // object    || enum is object | enum
/*pruned*/;                 // array     || enum is array | enum
/*pruned*/;                 // null      || enum is E
/*pruned*/;            // undefined || enum is E

/*pruned*/;                 // any       || object is any
let rh2 = a2 || a7;         // boolean   || object is boolean | object
let rh3 = a3 || a7;         // number    || object is number | object
let rh4 = a4 || a7;         // string    || object is string | object
/*pruned*/;                 // void      || object is void | object
/*pruned*/;                 // enum      || object is enum | object
let rh7 = a7 || a7;         // object    || object is object
let rh8 = a8 || a7;         // array     || object is array | object
let rh9 = null || a7;       // null      || object is object
let rh10 = null || a7; // undefined || object is object

/*pruned*/;                 // any       || array is any
let ri2 = a2 || a8;         // boolean   || array is boolean | array
let ri3 = a3 || a8;         // number    || array is number | array
let ri4 = a4 || a8;         // string    || array is string | array
/*pruned*/;                 // void      || array is void | array
/*pruned*/;                 // enum      || array is enum | array
let ri7 = a7 || a8;         // object    || array is object | array
let ri8 = a8 || a8;         // array     || array is array
let ri9 = null || a8;       // null      || array is array
let ri10 = null || a8; // undefined || array is array

/*pruned*/;                   // any       || null is any
let rj2 = a2 || null;         // boolean   || null is boolean
let rj3 = a3 || null;         // number    || null is number
let rj4 = a4 || null;         // string    || null is string
/*pruned*/;                   // void      || null is void
/*pruned*/;                   // enum      || null is E
let rj7 = a7 || null;         // object    || null is object
let rj8 = a8 || null;         // array     || null is array
let rj9 = null || null;       // null      || null is any
let rj10 = null || null; // undefined || null is any

/*pruned*/;                   // any       || undefined is any
let rf2 = a2 || null;         // boolean   || undefined is boolean
let rf3 = a3 || null;         // number    || undefined is number
let rf4 = a4 || null;         // string    || undefined is string
/*pruned*/;                   // void      || undefined is void
/*pruned*/;                   // enum      || undefined is E
let rf7 = a7 || null;         // object    || undefined is object
let rf8 = a8 || null;         // array     || undefined is array
let rf9 = null || null;       // null      || undefined is any
let rf10 = null || null; // undefined || undefined is any

function main(): void {}
