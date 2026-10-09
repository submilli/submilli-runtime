// @target: es2015
// @strict: true

let x: { a: 0 | 1 | undefined } = null as unknown as ({ a: 0 | 1 | undefined });

let { a: a1 } = x;
let { a: a2 = 0 } = x;
let { a: a3 = 2 } = x;
/*pruned*/;                    

let b1 = x.a;
let b2 = x.a ?? 0;
let b3 = x.a ?? 2;
/*pruned*/;                

// Repro from #35693

interface Foo {
  bar: 'yo' | 'ha' | undefined;
}

let { bar = 'yo' } = {} as Foo;

bar;  // "yo" | "ha"


function main(): void {}
