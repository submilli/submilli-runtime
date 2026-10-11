// @target: es2015
// @strict: true

const a: string | undefined = null as unknown as (string | undefined);
const b: string | undefined = null as unknown as (string | undefined);
const c: string | undefined = null as unknown as (string | undefined);

const foo1 = a ? 1 : 2;
const foo2 = a ?? 'foo' ? 1 : 2;
const foo3 = a ?? 'foo' ? (b ?? 'bar') : (c ?? 'baz');

function f (): void {
    const foo4 = a ?? 'foo' ? b ?? 'bar' : c ?? 'baz';
}


function main(): void {}
