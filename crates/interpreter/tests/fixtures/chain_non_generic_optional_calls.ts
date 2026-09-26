// Every non-generic optional method call compiles and runs in chain position —
// the positive half of the pair with `chain_generic_method_reference.ts`, which
// pins the two forms that must be rejected.

class Holder {
    plain(): number { return 7; }
}

export function main(): string {
    const a: number[] | null = [1, 2, 3];
    assert(a?.indexOf(2) === 1, "non-generic optional method call still works");
    assert(a?.join("-") === "1-2-3", "join works");

    const m: Map<string, number> | null = new Map<string, number>();
    m?.set("a", 1);
    assert(m?.get("a") === 1, "map set/get work through a chain");

    const b: Holder | null = new Holder();
    assert(b?.plain() === 7, "a user method works");

    const n: number[] | null = null as number[] | null;
    assert(n?.indexOf(2) === null, "a null receiver short-circuits");

    const s: string | null = "abc";
    assert(s?.toUpperCase() === "ABC", "a string method works");

    return "ok";
}
