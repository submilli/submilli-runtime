// expect-warning: constructor parameter `n` is undocumented
// expect-warning: method parameter `n` is undocumented
// expect-warning: parameter `n` is undocumented
// expect-warning: missing `@returns`
// expect-error-count: 7

/** Counter. */
export class Counter {
    /** Construct. */
    constructor(n: number) {}
    /** Run. */
    run(n: number): number { return n; }
    /** Static. */
    static run(n: number): number { return n; }
}
/** Arrow. */
export const arrow = (n: number): number => n;
/** Dotted names document object properties.
 * @param opts Options.
 * @param opts.a Value.
 * @param pair Pair.
 * @returns Sum.
 */
export function add(opts: {a: number}, {a, b}: {a: number; b: number}): number {
    return opts.a + a + b;
}
