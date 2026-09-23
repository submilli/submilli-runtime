// @target: es2015
// Note that type guards affect types of variables and parameters only and 
// have no effect on members of objects such as properties. 

// variables in global
let num: number = null as unknown as (number);
let var1: string | number = null as unknown as (string | number);
if (typeof var1 === "string") {
    num = var1.length; // string
}
else {
    num = var1; // number
}


function main(): void {}
