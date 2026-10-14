// Written for Submilli: no upstream case compares `typeof x` with "bigint".
// @strict: true

function describe(x: bigint | number): string {
    if (typeof x === "bigint") {
        let big = x;
        return (big * 2n).toString();
    }
    let rest = x;
    return rest.toFixed(1);
}

function wider(x: bigint | string | null): number {
    if (typeof x !== "bigint") {
        let notBig = x;
        return notBig === null ? -1 : notBig.length;
    }
    let big = x;
    return Number(big + 1n);
}

function loose(x: bigint | boolean): boolean {
    if ("bigint" == typeof x) {
        let big = x;
        return big > 0n;
    }
    let flag = x;
    return flag;
}

function unknownValue(x: unknown): bigint {
    if (typeof x === "bigint") {
        let big = x;
        return big;
    }
    return 0n;
}

let described = describe(10n) + describe(2);
let measured = wider(4n) + wider("abc") + wider(null);
let positive = loose(3n) && loose(true);
let recovered = unknownValue(5n);

function main(): void {}
