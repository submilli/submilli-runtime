// expect-error: expected `(arg0: number) => number`
function mk<F extends (n: number) => number>(read: F): number { return read(1); }
function main(): void { mk(n => { return "bad"; }); }
