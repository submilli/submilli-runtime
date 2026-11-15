// expect-error: expected `number`, got `number | null`
// A call in the condition invalidates the preceding module-variable guard.
import { invoke } from "@test/live";
let current:number|null=3;
function clear():boolean { current=null;return false; }
function main():void { const read=():number=>{if(current===null||clear())return 0;return current;};const actual:unknown=invoke(read);assert(actual===null); }
