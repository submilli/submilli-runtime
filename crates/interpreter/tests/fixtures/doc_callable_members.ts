// expect-warning: constructor parameter `n` is undocumented
// expect-warning: method parameter `n` is undocumented
// expect-warning: parameter `n` is undocumented
// expect-warning: missing `@returns`

class Counter {
    /** Construct. */
    constructor(n: number) {}
    /** Run. */
    run(n: number): number { return n; }
    /** Static. */
    static run(n: number): number { return n; }
}
/** Arrow. */
const arrow = (n: number): number => n;
/** Dotted names document object properties.
 * @param opts Options.
 * @param opts.a Value.
 * @param pair Pair.
 * @returns Sum.
 */
function add(opts: {a: number}, {a, b}: {a: number; b: number}): number {
    return opts.a + a + b;
}
function main(): number {
    return new Counter(1).run(2) + Counter.run(3) + arrow(4) + add({a: 5}, {a: 6, b: 7});
}
