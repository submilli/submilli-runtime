// A type guard renders as its `boolean` return: neither `Type::Function`'s
// `Display` nor the function-type grammar has a spelling for the predicate. The
// rendering *parses*, which is what makes it dangerous — pasted into an
// annotation it compiles and silently narrows nothing, and the loss surfaces much
// later as a failed read inside the `if` the guard was supposed to open. So the
// diagnostic that prints it says the rendering is lossy.
// expect-error: got `(arg0: number | string) => boolean`
// expect-error: this value is a type guard (`arg0 is number`), and the rendering above is lossy
// expect-error: Only a direct call of the guard narrows.
// The predicate need not be on the first parameter, and its asserted type need
// not be a primitive.
// expect-error: this value is a type guard (`arg1 is number`)
// expect-error: this value is a type guard (`arg0 is Map<string, number>`)
// The note is a `help:` entry of its own, so it still carries a gutter when a
// structural diff is printed beside it.
// expect-error: return: expected `number`, got `boolean`

function isNum(x: number | string): x is number { return typeof x === "number"; }
function isSecond(tag: string, x: number | string): x is number { return typeof x === "number"; }
function isMap(v: Map<string, number> | null): v is Map<string, number> { return v !== null; }
function take(n: number): string { return n.toString(); }
function wants(f: (a: number | string) => number): number { return f(1); }

export function main(): string {
    // Assignment position.
    const bad: number = isNum;
    // Argument position, with a structural diff alongside the note.
    const alsoBad = wants(isNum);
    // The other two guards, to pin the parameter index and the asserted type.
    const second: number = isSecond;
    const mapped: number = isMap;
    return take(isNum);
}
