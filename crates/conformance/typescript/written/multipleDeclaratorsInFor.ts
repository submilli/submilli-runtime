// Written for Submilli: no upstream case Submilli can run declares several
// variables in a `for` head or lists several expressions in a `for` initializer
// or update.

function reverse(xs: number[]): number[] {
    for (let lo = 0, hi = xs.length - 1; lo < hi; lo++, hi--) {
        const t = xs[lo];
        xs[lo] = xs[hi];
        xs[hi] = t;
    }
    return xs;
}

function sum(xs: number[]): number {
    let total = 0;
    for (let i = 0, n = xs.length, label = "sum"; i < n; i++) {
        total += label.length > 0 ? xs[i] : 0;
    }
    return total;
}

function steps(limit: number): string {
    let i = -1, s = "?";
    for (i = 0, s = ""; i < limit; i++, s += "x") {}
    return s;
}

function main(): void {
    const reversed = reverse([1, 2, 3]);
    const total = sum(reversed);
    const trail = steps(total);
}
