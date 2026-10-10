// @target: es2015
// @noImplicitAny: true
interface Show {
    show: (x: number) => string;
}
function f({ show: showRename = v => v }: Show): void {}
/*pruned*/;
/*pruned*/;

interface Nested {
    nested: Show
}
function ff({ nested: nestedRename = { show: v => v } }: Nested): void {}

interface StringIdentity {
    stringIdentity(s: string): string;
}
let { stringIdentity: id = arg => arg.length }: StringIdentity = { stringIdentity: x => x};

interface Tuples {
    prop: [string, number];
}
function g({ prop = [101, 1234] }: Tuples): void {}

interface StringUnion {
    prop: "foo" | "bar";
}
function h({ prop = "baz" }: StringUnion): void {}


function main(): void {}
