// @target: es2015
let numStrTuple: [number, string] = null as unknown as ([number, string]);
let numNumTuple: [number, number] = null as unknown as ([number, number]);
let numEmptyObjTuple: [number, {}] = null as unknown as ([number, {}]);
let emptyObjTuple: [{}] = null as unknown as ([{}]);

let numArray: number[] = null as unknown as (number[]);
let emptyObjArray: {}[] = null as unknown as ({}[]);

// no error
numArray = numNumTuple;
emptyObjArray = emptyObjTuple;
emptyObjArray = numStrTuple;
emptyObjArray = numNumTuple;
emptyObjArray = numEmptyObjTuple;

// error
numArray = numStrTuple;
emptyObjTuple = emptyObjArray;


function main(): void {}
