// expect-error: every field of `Weak` is optional
// expect-error: must share at least one field with such a type
// expect-error-count: 1
interface Weak { a?: number; }
function main(): void {
    const unrelated = { b: 1 };
    const target: Weak = unrelated;
}
