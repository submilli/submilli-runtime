import { factory } from "@test/live";
function main(): void { const read=factory(); const actual:unknown=read(); assert(actual===null); }
