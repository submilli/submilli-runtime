// @target: es2015
let x: StringTree = null as unknown as (StringTree);
if (typeof x !== "string") {
    x.push("");
    x.push([""]);
}

type StringTree = string | StringTreeArray;
interface StringTreeArray extends Array<StringTree> { }

function main(): void {}
