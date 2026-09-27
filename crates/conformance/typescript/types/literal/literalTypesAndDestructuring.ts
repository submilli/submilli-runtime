// @target: es2015
// @strict: true

let x: { a: 0 | 1 | null } = null as unknown as ({ a: 0 | 1 | null });

let { a: a1 } = x;
/*pruned*/;           
/*pruned*/;           
/*pruned*/;                    

let b1 = x.a;
let b2 = x.a ?? 0;
let b3 = x.a ?? 2;
/*pruned*/;                

// Repro from #35693

interface Foo {
  bar: 'yo' | 'ha' | null;
}

/*pruned*/;                    

;     // "yo" | "ha"


function main(): void {}
