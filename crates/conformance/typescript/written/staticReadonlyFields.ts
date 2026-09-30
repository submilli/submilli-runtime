// Written for Submilli: the upstream cases with a `static readonly` field that
// Submilli can run give it no value it checks.

class Limits {
    static readonly max: number = 5 * 2;
    static readonly label: string = `limit ${Limits.max}`;
    static count: number = Limits.max - 1;
}

let max = Limits.max;
let label = Limits.label;
Limits.count += 1;
Limits.max = 11;

function main(): void {}
