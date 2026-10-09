// @target: es2015
// @noImplicitAny: true
interface Show {
    show: (x: number) => string;
}
function f({ show = v => v.toString() }: Show): void {}
/*pruned*/;                                                           
/*pruned*/;                                                             

interface Nested {
    nested: Show
}
function ff({ nested = { show: v => v.toString() } }: Nested): void {}

interface Tuples {
    prop: [string, number];
}
function g({ prop = ["hello", 1234] }: Tuples): void {}

interface StringUnion {
    prop: "foo" | "bar";
}
function h({ prop = "foo" }: StringUnion): void {}

interface StringIdentity {
    stringIdentity(s: string): string;
}
/*pruned*/;                                                                         




function main(): void {}
