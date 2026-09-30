// Written for Submilli: the upstream cases with `finally` that Submilli can run
// leave its blocks empty, so nothing in them is checked.

function total(values: number[]): number {
    let sum = 0;
    let attempts = 0;
    try {
        for (const value of values) sum += value;
    } finally {
        attempts = attempts + 1;
    }
    return attempts > 0 ? sum : -1;
}

function divide(a: number, b: number): number | null {
    let result: number | null = null;
    try {
        if (b === 0) throw new RangeError("division by zero");
        result = a / b;
    } catch (e) {
        result = null;
    } finally {
        let attempted: string = `${a} / ${b}`;
    }
    return result;
}

function first(values: string[]): string {
    try {
        return values[0];
    } finally {
        let count = values.length;
    }
}

function main(): void {}
