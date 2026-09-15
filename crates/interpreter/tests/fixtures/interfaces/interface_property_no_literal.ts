// Regression: a field read on an `interface`-typed value, when no object
// literal of that shape exists in the module, must still get a per-name
// `$string` global. Codegen previously panicked emitting `o.engine` because
// the name was collected only from object literals and VTable interfaces. The
// exported function is emitted (library surface) though `main` never calls it.

interface Opt {
    engine: string;
    timeout?: number;
}

export function engineOf(o: Opt): string {
    return o.engine;
}

function main(): void {
    assert(true, "compiles without a per-name string-global panic");
}
