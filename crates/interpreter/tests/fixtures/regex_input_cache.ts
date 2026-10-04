function main(): number {
    const a = "x".repeat(128);
    const b = "y".repeat(256);
    const r = /x/g;
    for (let i = 0; i < a.length; i++) {
        const hit = r.exec(a);
        if (hit === null) { throw new Error("missing match"); }
        if (hit.index !== i || hit.input !== a || hit.match !== "x") {
            throw new Error("cached input changed result");
        }
    }
    if (r.exec(a) !== null || r.lastIndex !== 0) { throw new Error("end of input"); }
    if (r.test(b)) { throw new Error("stale input"); }
    if (!r.test(a) || (r.lastIndex as number) !== 1) { throw new Error("evicted input"); }
    const other = /y/g;
    if (!other.test(b)) { throw new Error("second regex"); }
    const next = r.exec(a);
    if (next === null || next.index !== 1) { throw new Error("lastIndex lost"); }
    return 0;
}
