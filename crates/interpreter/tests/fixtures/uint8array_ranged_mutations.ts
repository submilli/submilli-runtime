function main(): void {
    for (const size of [128, 256]) {
        const bytes = Uint8Array.alloc(size);
        for (let i=0; i<size; i++) { bytes.fill(i % 128, i, i+1); }
        for (let i=0; i<size; i++) { assert(bytes[i] === i % 128); }
        bytes.copyWithin(1, 0, size-1);
        assert(bytes[1] === 0 && bytes[size-1] === (size-2) % 128);
        bytes.copyWithin(0, 1, size);
        assert(bytes[0] === 0 && bytes[size-2] === (size-2) % 128);
        const one = Uint8Array.new([7]);
        for (let i=0; i<size; i++) { bytes.set(one, i); }
        assert(bytes.every((value: number): boolean => value === 7));
        bytes.set(bytes, 0);
        bytes.fill(99, size, size);
        bytes.copyWithin(size, 0, size);
        let caught = false;
        try { bytes.set(one, size); } catch (error: Error) { caught = error instanceof RangeError; }
        assert(caught && bytes[size-1] === 7);
    }
    assert(Uint8Array.alloc(0).length === 0);
    const zeroes = new Uint8Array(256);
    assert(zeroes.every((value: number): boolean => value === 0));
}
