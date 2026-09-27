// @target: es2015
let x: string | boolean | number = null as unknown as (string | boolean | number);
/*pruned*/;                             

x = "";
x = x.length;
x; // number

x = true;
/*pruned*/;                        
x; // number

// https://github.com/microsoft/TypeScript/issues/35484
type D = { done: true, value: 1 } | { done: false, value: 2 };
function fn(): D { return null as unknown as (D); }
let o: D = null as unknown as (D);
if ((o = fn()).done) {
    const y: 1 = o.value;
}

function main(): void {}
