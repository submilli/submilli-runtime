interface Empty {}
interface Length { length: number }
function length(value: Length): number { return value.length; }
function truth(value: Empty): boolean { return !!value; }
interface HasX { x: number }
function required(value: HasX): number { return value && 1; }
function empty(): Empty { return 0; }
function main(): void {
    assert(required({ x: 1 }) === 1);
    const value = empty() && { a: 1 };
    assert(JSON.stringify(value) === "0");
    assert(value.a === undefined);
    assert(value?.a === undefined);
    const nested = empty() && { a: { b: 3 } };
    assert(nested.a?.b === undefined);
    assert(nested?.a?.b === undefined);
    const callable = empty() && { f: (): number => 3 };
    assert(callable.f?.() === undefined);
    assert(callable?.f?.() === undefined);
    assert((empty() || 7) === 7);
    assert(!truth(0));
    assert(!truth(false));
    assert(!truth(""));
    assert(truth(1));
    assert(truth("x"));
    assert(truth({}));
    assert(length("") === 0);
    assert(length("abc") === 3);
    assert(length({ length: 7 }) === 7);
}
