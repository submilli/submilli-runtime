// Every annotation here is the verbatim rendering a diagnostic prints for the
// corresponding function type (see `function_type_renders_parseable.ts`), so
// this fixture fails the moment a rendering stops being pasteable.

function pick(a: number, b: string): string { return b + a.toString(); }
function collect(head: number, ...tail: number[]): void { assert(head + tail.length >= 0, "counted"); }

export function main(): string {
    const roundPlain: (arg0: number, arg1: string) => string = pick;
    assert(roundPlain(1, "a") === "a1", "named function type parses and runs");

    const roundRest: (arg0: number, ...arg1: number[]) => void = collect;
    roundRest(1, 2, 3);

    const roundArray: ((arg0: string) => number)[] = [(s: string): number => s.length];
    assert(roundArray[0]("abc") === 3, "parenthesized function element parses and runs");

    const roundNullary: () => string = (): string => "n";
    assert(roundNullary() === "n", "nullary function type parses and runs");

    return "ok";
}
