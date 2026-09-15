// A generic method *reference* in a chain is permanently illegal, exactly like a
// non-generic one — it must say so rather than "not yet supported", which tells a
// model to wait for a release that will never accept it. The *call* form is the
// one that really is unimplemented, and keeps that message.
// expect-error: method `map` must be called — a method reference is not a value
// expect-error: method `keys` must be called — a method reference is not a value
// expect-error: optional method `map` with method-level generics is not yet supported in chain position
// expect-error: method `reduce` must be called — a method reference is not a value

export function main(): string {
    const a: number[] | null = [1, 2];
    // `Array#map` has its own type parameter `U`, so it takes the deferred path.
    const g = a?.map;

    // `Map#keys` is generic only in its *interface* parameters, so it always
    // reached the reference message; it holds that half of the pair in place.
    const m: Map<string, number> | null = new Map<string, number>();
    const k = m?.keys;

    // Another method-level-generic prelude method, to pin that the reference
    // path is the method's own property and not something special about `map`.
    const r = a?.reduce;

    // The call form: genuinely unimplemented, and still says so.
    const called = a?.map((x: number): number => x * 2);
    return "x";
}
