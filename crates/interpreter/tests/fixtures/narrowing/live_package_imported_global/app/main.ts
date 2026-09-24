import { current, clear } from "@test/live";
function main(): void { if(current === null || clear()) return; const actual:unknown=current; assert(actual===null); }
