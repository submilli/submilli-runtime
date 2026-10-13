function main(): void {
    const lone = String.fromCharCode(0xD800);
    const pad = String.fromCharCode(0xDC00) + "x";
    assert(lone.repeat(2).charCodeAt(1) === 0xD800);
    assert(lone.repeat(2.9).length === 2);
    assert(lone.repeat(-0.5) === "");
    assert(lone.repeat(NaN) === "");
    assert("".repeat(Infinity) === "");
    assert(lone.concat(pad).charCodeAt(0) === 0xD800);
    assert((lone + pad).charCodeAt(1) === 0xDC00);
    assert(lone.padStart(4, pad) === pad + String.fromCharCode(0xDC00) + lone);
    assert(lone.padEnd(4, pad) === lone + pad + String.fromCharCode(0xDC00));
    assert(lone.padStart(-1, pad) === lone);
    assert(lone.padEnd(NaN, pad) === lone);
    assert(lone.padStart(Infinity, "") === lone);
    let caught = 0;
    try { const bad = lone.repeat(-1); } catch (e: Error) {
        assert(e instanceof RangeError);
        caught++;
    }
    try { const bad = lone.repeat(Infinity); } catch (e: Error) {
        assert(e instanceof RangeError);
        caught++;
    }
    try { const bad = lone.padStart(33554433, pad); } catch (e: Error) {
        assert(e instanceof RangeError);
        caught++;
    }
    try { const bad = lone.padEnd(Infinity, pad); } catch (e: Error) {
        assert(e instanceof RangeError);
        caught++;
    }
    assert(caught === 4);

    // Sources longer than the host's 2048-unit copy buffer are streamed and
    // restart at their first unit; short ones repeat across buffer blocks.
    let long = "";
    for (let i = 0; i < 5000; i++) {
        long += String.fromCharCode(65 + (i % 26));
    }
    const twice = long.repeat(2);
    assert(twice.length === 10000);
    assert(twice.slice(0, 5000) === long);
    assert(twice.slice(5000) === long);
    assert(long + long === twice);
    assert(long.concat(long) === twice);
    const started = "z".padStart(7001, long);
    assert(started.slice(0, 5000) === long);
    assert(started.slice(5000, 7000) === long.slice(0, 2000));
    assert(started.charCodeAt(7000) === 122);
    const ended = "z".padEnd(12000, "abc");
    assert(ended.length === 12000);
    for (let i = 1; i < 12000; i++) {
        assert(ended.charCodeAt(i) === "abc".charCodeAt((i - 1) % 3));
    }
    assert("ab".repeat(3000).slice(4094, 4100) === "ababab");
    for (const size of [2048, 2049]) {
        const source = long.slice(0, size);
        const doubled = source.repeat(2);
        assert(doubled.length === 2 * size);
        assert(doubled.slice(size) === source);
        assert("".padEnd(size + 7, source) === source + source.slice(0, 7));
    }
}
