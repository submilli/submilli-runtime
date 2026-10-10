// Host string and byte operations copy whole payloads at once. Every code unit
// and byte value must survive the copy, including lone surrogates and the
// values whose top bit is set.
function units(s: string): number[] {
    const out: number[] = [];
    for (let i = 0; i < s.length; i++) {
        out.push(s.charCodeAt(i));
    }
    return out;
}

function sameUnits(actual: string, expected: number[], label: string): void {
    const got = units(actual);
    assert(got.length === expected.length, label + ": length");
    for (let i = 0; i < expected.length; i++) {
        assert(got[i] === expected[i], label + ": unit " + i.toString());
    }
}

function main(): void {
    const edges = [0x0000, 0x0041, 0x7fff, 0x8000, 0xd800, 0xdbff, 0xdc00, 0xdfff, 0xfffe, 0xffff];
    let text = "";
    for (const unit of edges) {
        text = text.concat(String.fromCharCode(unit));
    }
    sameUnits(text, edges, "concat one unit at a time");
    sameUnits(text.slice(0), edges, "slice");
    sameUnits(text.repeat(2).slice(edges.length), edges, "repeat");
    sameUnits(text.padStart(edges.length + 1, String.fromCharCode(0xd800)).slice(1), edges, "padStart");
    sameUnits(text.slice(4, 5), [0xd800], "lone high surrogate");
    assert(text.indexOf(String.fromCharCode(0xdfff)) === 7, "indexOf a lone low surrogate");
    assert(text.indexOf("￿", 9) === 9, "indexOf from the last unit");
    assert(text.startsWith("￾￿", 8), "startsWith at a position");

    let appended = "";
    for (const unit of edges) {
        appended += String.fromCharCode(unit);
    }
    sameUnits(appended, edges, "+= one unit at a time");

    // A search that resumes from each match covers a long string once.
    const page = "The quick brown fox jumps over the lazy dog. ".repeat(2000);
    let count = 0;
    let at = page.indexOf("fox");
    while (at !== -1) {
        count += 1;
        at = page.indexOf("fox", at + 1);
    }
    assert(count === 2000, "indexOf from each match finds every fox");

    // In-place Uint8Array mutators write bytes back into the same backing.
    const bytes = Uint8Array.new([0x00, 0x7f, 0x80, 0xfe, 0xff]);
    const alias = bytes;
    bytes.reverse();
    assert(alias[0] === 0xff && alias[1] === 0xfe && alias[2] === 0x80 && alias[4] === 0x00, "reverse");
    bytes.fill(0x80, 1, 3);
    assert(bytes[0] === 0xff && bytes[1] === 0x80 && bytes[2] === 0x80 && bytes[3] === 0x7f, "fill a range");
    bytes.copyWithin(0, 3);
    assert(bytes[0] === 0x7f && bytes[1] === 0x00 && bytes[2] === 0x80, "copyWithin");
    bytes.set(Uint8Array.new([0xaa, 0xbb]), 3);
    assert(bytes[3] === 0xaa && bytes[4] === 0xbb && bytes[0] === 0x7f, "set at an offset");
    bytes.sort();
    assert(bytes[0] === 0x00 && bytes[1] === 0x7f && bytes[2] === 0x80 && bytes[4] === 0xbb, "sort");
    assert(alias.length === 5 && alias[4] === 0xbb, "the alias sees every write");
    assert(bytes.toBase64() === "AH+Aqrs=", "bytes read back for toBase64");
}
