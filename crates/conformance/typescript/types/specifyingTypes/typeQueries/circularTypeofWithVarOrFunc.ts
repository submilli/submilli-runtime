// @target: es2015
// @strict: true
type typeAlias1 = typeof varOfAliasedType1;
let varOfAliasedType1: typeAlias1 = null as unknown as (typeAlias1);

let varOfAliasedType2: typeAlias2 = null as unknown as (typeAlias2);
type typeAlias2 = typeof varOfAliasedType2;

function func(): typeAlias3 { return null; }
let varOfAliasedType3 = func();
type typeAlias3 = typeof varOfAliasedType3;

// Repro from #26104

interface Input {
  a: number;
  b: number;
}

type R = ReturnType<typeof mul>;
function mul(input: Input): R {
  return input.a * input.b;
}

// Repro from #26104

type R2 = ReturnType<typeof f>;
function f(): R2 { return 0; }


function main(): void {}
