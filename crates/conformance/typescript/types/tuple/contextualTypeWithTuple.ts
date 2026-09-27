// @target: es2015
// no error
let numStrTuple: [number, string] = [5, "hello"];
let numStrTuple2: [number, string] = [5, "foo", true];
let numStrBoolTuple: [number, string, boolean] = [5, "foo", true];
let objNumTuple: [{ a: string }, number] = [{ a: "world" }, 5];
let strTupleTuple: [string, [number, {}]] = ["bar", [5, { x: 1, y: 1 }]];
class C { }
class D { }
let unionTuple: [C, string | number] = [new C(), "foo"];
let unionTuple1: [C, string | number] = [new C(), "foo"];
let unionTuple2: [C, string | number, D] = [new C(), "foo", new D()];
let unionTuple3: [number, string| number] = [10, "foo"]; 

numStrTuple = numStrTuple2;
numStrTuple = numStrBoolTuple;

// error
objNumTuple = [ {}, 5];
numStrBoolTuple = numStrTuple;
let strStrTuple: [string, string] = ["foo", "bar", 5];

unionTuple = unionTuple1;
unionTuple = unionTuple2;
unionTuple2 = unionTuple;
numStrTuple = unionTuple3;

// repro from #29311
/*pruned*/;               
/*pruned*/;                        
/*pruned*/;              

// #52551
/*pruned*/;         
/*pruned*/;                                                   
/*pruned*/;                       


function main(): void {}
