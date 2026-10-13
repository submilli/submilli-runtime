// @target: es2015
// @noImplicitAny: true
enum E {
    A = "a",
    B = "b",
    C = "c",
}

interface Item {
    a: string;
    b: number;
    c: boolean;
}

const item: Item = null as unknown as (Item);
const e: E = null as unknown as (E);
const snb: string | number | boolean = item[e];


function main(): void {}
