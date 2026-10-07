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
}
