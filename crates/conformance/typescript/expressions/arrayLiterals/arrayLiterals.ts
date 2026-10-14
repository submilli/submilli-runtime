// @target: es2015
// @strict: true
// Empty array literal with no contextual type has type Undefined[]

let arr1= [[], [1], ['']];

let arr2 = [[null], [1], ['']];


// Array literal with elements of only EveryType E has type E[]
let stringArrArr = [[''], [""]];

let stringArr = ['', ""];

let numberArr = [0, 0.0, 0x00, 1e1];

let boolArr = [false, true, false, true];

/*pruned*/;           
/*pruned*/;                       

/*pruned*/;                    
/*pruned*/;                                                                   // Should OK, not be a parse error

// Contextual type C with numeric index signature makes array literal of EveryType E of type BCT(E,C)[]
/*pruned*/;                                                                                                 
let context2 = [{ a: '', b: 0, c: '' }, { a: "", b: 3, c: 0 }];

// Contextual type C with numeric index signature of type Base makes array literal of Derived have type Base[]
/*pruned*/;              
/*pruned*/;                              ;
/*pruned*/;                              ;
/*pruned*/;                                             

// Contextual type C with numeric index signature of type Base makes array literal of Derived1 and Derived2 have type Base[]
/*pruned*/;                                             



function main(): void {}
