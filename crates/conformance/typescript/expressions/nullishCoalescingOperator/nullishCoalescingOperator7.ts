// @target: es2015
// @strict: true

const a: string | null = null as unknown as (string | null);
const b: string | null = null as unknown as (string | null);
const c: string | null = null as unknown as (string | null);

const foo1 = a ? 1 : 2;
const foo2 = a ?? 'foo' ? 1 : 2;
const foo3 = a ?? 'foo' ? (b ?? 'bar') : (c ?? 'baz');

function f (): void {
    const foo4 = a ?? 'foo' ? b ?? 'bar' : c ?? 'baz';
}


function main(): void {}
