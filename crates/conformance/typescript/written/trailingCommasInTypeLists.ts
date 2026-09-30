// Written for Submilli: no upstream case Submilli can run puts a trailing comma
// in a type argument, type parameter or function-type parameter list.

function pair<A, B,>(first: A, second: B,): [A, B] {
    return [first, second];
}

let explicit = pair<number, string,>(1, "one",);
let inferred = pair(true, 2,);

let handler: (event: string, count: number,) => number = (event: string, count: number): number => event.length + count;
let handled = handler("click", 1,);

class Box<T,> {
    constructor(public value: T,) {}
}

let box = new Box<string,>("a",);
let boxed = box.value;

function main(): void {}
