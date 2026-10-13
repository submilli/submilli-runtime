// Case maps and trims read the receiver through 2048-unit windows and write
// the result in place; these cross the window edges and change lengths.
function main(): void {
    const pair = String.fromCharCode(0xD801, 0xDC00); // 𐐀, lowercases to 𐐨
    const lowerPair = String.fromCharCode(0xD801, 0xDC28);
    const split = "a".repeat(2047) + pair + "b";
    assert(split.toLowerCase() === "a".repeat(2047) + lowerPair + "b");
    assert(split.toUpperCase() === "A".repeat(2047) + pair + "B");

    const lone = String.fromCharCode(0xD800);
    const edge = "x".repeat(2047) + lone;
    assert(edge.toUpperCase() === "X".repeat(2047) + lone);
    assert((lone + "a").toUpperCase() === lone + "A");

    // Longer results: ß → SS, İ → i̇.
    const sharp = "ß".repeat(3000);
    assert(sharp.toUpperCase() === "SS".repeat(3000));
    assert(("a" + "ß".repeat(5000) + "z").toUpperCase() === "A" + "SS".repeat(5000) + "Z");
    const dotted = "İ".repeat(2500).toLowerCase();
    assert(dotted.length === 5000);
    assert(dotted === "i̇".repeat(2500));

    // Final sigma looks ahead across ignorable characters.
    assert("ΑΣ".toLowerCase() === "ας");
    assert("ΑΣΑ".toLowerCase() === "ασα");
    assert(("Α" + "Σ" + "'".repeat(3000)).toLowerCase() === "α" + "ς" + "'".repeat(3000));
    assert(("Α" + "Σ" + "'".repeat(3000) + "Α").toLowerCase() === "α" + "σ" + "'".repeat(3000) + "α");

    const padded = " \t".repeat(3000) + "mid" + "　\n".repeat(3000);
    assert(padded.trim() === "mid");
    assert(padded.trimStart() === "mid" + "　\n".repeat(3000));
    assert(padded.trimEnd() === " \t".repeat(3000) + "mid");
    const blank = " ".repeat(5000);
    assert(blank.trim() === "");
    assert(blank.trimStart() === "");
    assert(blank.trimEnd() === "");
    assert("".trim() === "" && "".toUpperCase() === "" && "".toLowerCase() === "");
    const word = "w".repeat(4100);
    assert(word.trim() === word);

    // Window edges at 2047/2048, 3x growth, and a pair split across the point
    // where the result outgrows the receiver.
    const low = String.fromCharCode(0xDC00);
    assert(("a".repeat(2048) + lone).toUpperCase() === "A".repeat(2048) + lone);
    assert(("a".repeat(2047) + low + "b").toUpperCase() === "A".repeat(2047) + low + "B");
    assert("\u0390".repeat(3000).toUpperCase() === "\u0399\u0308\u0301".repeat(3000));
    assert(("a".repeat(2046) + "ß" + pair).toLowerCase() === "a".repeat(2046) + "ß" + lowerPair);
    assert(("a".repeat(2046) + "ß" + pair).toUpperCase() === "A".repeat(2046) + "SS" + pair);
    assert(("Α".repeat(2047) + "Σ").toLowerCase() === "α".repeat(2047) + "ς");
    assert(("Α".repeat(2048) + "Σ" + "Α").toLowerCase() === "α".repeat(2048) + "σα");
    for (const size of [2048, 2049, 4096]) {
        assert("q".repeat(size).toUpperCase() === "Q".repeat(size));
        assert("Q".repeat(size).toLowerCase() === "q".repeat(size));
    }
}
