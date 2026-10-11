// @target: es2015
function f1(x: number): string { return "foo"; }

function f2(x: number): number { return 10; }

function f3(x: number): boolean { return true; }

enum E1 { one }

enum E2 { two }


let t1: [(x: number) => string, (x: number) => number] = null as unknown as ([(x: number) => string, (x: number) => number]);
/*pruned*/;                                      
/*pruned*/;                                                
/*pruned*/;                                                      

// no error
t1 = [f1, f2];
/*pruned*/;           
/*pruned*/;         
/*pruned*/;               
let e1 = t1[2];  // {}
/*pruned*/;      // {}
/*pruned*/;      // any
/*pruned*/;      // number

function main(): void {}
