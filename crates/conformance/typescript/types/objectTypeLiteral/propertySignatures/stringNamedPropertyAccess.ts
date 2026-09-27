// @target: es2015
class C {
    "a b": number;
    static "c d": number;
}
/*pruned*/;                       
/*pruned*/;       
let r1b = C['c d'];

interface I {
    "a b": number;
}
let i: I = null as unknown as (I);
let r2 = i["a b"];

let a: {
    "a b": number;
} = null as unknown as ({
    "a b": number;
});
let r3 = a["a b"];

let b = {
    "a b": 1
}
let r4 = b["a b"];

function main(): void {}
