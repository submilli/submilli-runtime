// A lifted method signature renders with the bindings member *resolution*
// produced. The prelude receivers are the ones with no other coverage: the unit
// tests below `format_signature` hand the formatter a table directly, so nothing
// but this pins the routing that builds it. A `find_method` change would print
// `Map<string, number>.get(key: K): V | undefined` and no test would fail.
// expect-error: Map<string, number>.get(key: string): number | undefined
// expect-error: number[].map<U>(callback: (arg0: number, arg1: number, arg2: number[]) => U): U[]
// expect-error: string.repeat(count: number): string
// expect-error: Set<number>.add(value: number): Set<number>

export function main(): string {
    const m = new Map<string, number>();
    m.get();

    const arr: number[] = [1];
    arr.map();

    // Annotated, so the receiver renders as `string`; an unannotated `const`
    // infers the literal type and would render `"hi"`.
    const s: string = "hi";
    s.repeat();

    const st = new Set<number>();
    st.add();

    return "x";
}
