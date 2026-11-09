// @target: es2015
function f1(x: number): string { return "foo"; }

function f2(x: number): number { return 10; }

function f3(x: number): boolean { return true; }

enum E1 { one }

enum E2 { two }


let t1: [(x: number) => string, (x: number) => number] = null as unknown as ([(x: number) => string, (x: number) => number]);
let t2: [E1, E2] = null as unknown as ([E1, E2]);
/*pruned*/;                                                
let t4: [E1, E2, number] = null as unknown as ([E1, E2, number]);

// no error
t1 = [f1, f2];
t2 = [E1.one, E2.two];
/*pruned*/;         
t4 = [E1.one, E2.two, 20];
let e1 = t1[2];  // {}
let e2 = t2[2];  // {}
/*pruned*/;      // any
let e4 = t4[3];  // number

function main(): void {}
