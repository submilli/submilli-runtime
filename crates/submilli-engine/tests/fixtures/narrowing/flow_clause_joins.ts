function switchJoin(k: number): string {
 let s: string | null = null;
 switch(k) { case 0: s = "a"; break; default: s = "b"; break; }
 return s.toUpperCase();
}
function risky(fail: boolean): string { if(fail) { throw new Error("boom"); } return "t"; }
function tryJoin(fail: boolean): string { let s: string | null = null;
 try { s = risky(fail); } catch(e) { s = "c"; } return s.toUpperCase();
}
function finallyJoin(fail: boolean): string { let s: string | null = null;
 try { s = risky(fail); } catch(e) { s = "c"; } finally { s = "f"; } return s.toUpperCase();
}
function catchReturns(fail: boolean): string { let s: string | null = null;
 try { s = risky(fail); } catch(e) { return "C"; } return s.toUpperCase();
}
function main(): void {
 assert(switchJoin(0) === "A", "case assignment"); assert(switchJoin(1) === "B", "default assignment");
 assert(tryJoin(false) === "T", "normal try exit"); assert(tryJoin(true) === "C", "catch exit");
 assert(finallyJoin(false) === "F", "finally after body"); assert(finallyJoin(true) === "F", "finally after catch");
 assert(catchReturns(false) === "T", "only completing clause joins"); assert(catchReturns(true) === "C", "returning catch");
}
