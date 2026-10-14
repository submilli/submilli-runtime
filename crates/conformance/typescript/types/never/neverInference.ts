// @lib: es2015
// @strict: true
// @target: es2015

/*pruned*/;                                                 

let neverArray: never[] = [];

/*pruned*/;       // never
/*pruned*/;               // never

// Repro from #19576

type Comparator<T> = (x: T, y: T) => number;

/*pruned*/;              
                              
                  
 

/*pruned*/;                                      

function compareNumbers(x: number, y: number): number { return null as unknown as (number); }
/*pruned*/;                                                                                                            

/*pruned*/;                                                 

// Repro from #19858

function f2<a>(as1: a[], as2: a[], cmp: (a1: a, a2: a) => number): void { }
f2(Array.from([0]), [], (a1, a2) => a1 - a2);
f2(Array.from([]), [0], (a1, a2) => a1 - a2);


function main(): void {}
