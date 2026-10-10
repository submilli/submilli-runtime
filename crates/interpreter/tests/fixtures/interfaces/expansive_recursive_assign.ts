interface SwapLeft<A, B> { next: SwapLeft<B, A[]> | null }
interface SwapRight<A, B> { next: SwapRight<B, A[]> | null }
interface OneSide<X> { next: OneSide<X[]> | null }
interface Stable<X> { next: Stable<X> | null }
interface FunctionLeft<X> { value: X; next: FunctionLeft<() => X> | null }
interface FunctionRight<X> { value: X; next: FunctionRight<() => X> | null }
interface Growing<X> { readonly value: X; next: Growing<X[]> | null }
interface Mutable<X> { value: X; next: Mutable<X[]> | null }
interface Nested<X> { readonly value: X; next: Nested<Nested<X>> | null }
interface Other<X> { value: X; next: Other<Other<X>> | null }
function read(value: Other<number>): number { return value.value; }
function main(): void {
    const swapped: SwapLeft<number, string> = { next: null };
    const swappedTarget: SwapRight<number, string> = swapped;
    assert(swappedTarget.next === null, "alternating argument growth");
    const oneSide: OneSide<number> = { next: null };
    const stable: Stable<number> = oneSide;
    assert(stable.next === null, "one-sided expansion");
    const functionLeft: FunctionLeft<number> = { value: 3, next: null };
    const functionRight: FunctionRight<number> = functionLeft;
    assert(functionRight.value === 3, "function-wrapped expansion");
    const source: Growing<number> = { value: 1, next: null };
    const target: Mutable<number> = source;
    assert(target.value === 1, "expanding array arguments");
    let nullable: Mutable<number> | null = null;
    nullable = source;
    assert(nullable.value === 1, "union target");
    const nested: Nested<number> = { value: 2, next: null };
    assert(read(nested) === 2, "expanding named arguments");
}
