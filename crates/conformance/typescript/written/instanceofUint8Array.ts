// Written for Submilli: `instanceof Uint8Array` is the one `instanceof` the spec
// allows on something other than a class, and no upstream case uses it.

function size(value: Uint8Array | string): number {
    if (value instanceof Uint8Array) {
        let bytes = value;
        return bytes.length;
    }
    let text = value;
    return text.length;
}

let buffer = new Uint8Array(4);
let isBytes = buffer instanceof Uint8Array;
let measured = size(buffer) + size("four");

function main(): void {}
