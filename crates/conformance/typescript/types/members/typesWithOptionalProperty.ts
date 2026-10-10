// @target: es2015
// basic uses of optional properties without errors

interface I {
    foo: string;
    bar?: number;
    baz? (): string;
}

let a: {
    foo: string;
    bar?: number;
    baz? (): string;
} = null as unknown as ({
    foo: string;
    bar?: number;
    baz? (): string;
});

let b = { foo: '' };
let c = { foo: '', bar: 3 };
let d = { foo: '', bar: 3, baz: () => '' };

let i: I = null as unknown as (I);

i = b;
i = c;
i = d;

a = b;
a = c;
a = d;

i = a;
a = i;

function main(): void {}
