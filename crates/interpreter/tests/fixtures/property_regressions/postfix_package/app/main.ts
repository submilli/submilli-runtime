import { Counter, inc, dec, OptionalValue, OptionalBase, hasOptional } from "@test/postfix";
class Stored implements Counter {
    private held: bigint = 10n;
    get value(): bigint { return this.held; }
    set value(value: bigint) { this.held = value; }
}
class ReadOnly { get value(): bigint { return 1n; } }
class Child extends OptionalBase {}
function main(): void {
    const absent: OptionalValue = {};
    assert(!hasOptional(absent), "optional name metadata crosses packages");
    assert(hasOptional({optional:null}), "required null crosses packages");
    assert(!hasOptional(new Child()), "imported inherited optional data field");
    const counter: Counter = new Stored();
    assert(inc(counter) === 10n, "library postfix invokes consumer bigint accessor");
    assert(dec(counter) === 11n, "library decrement invokes consumer accessor");
    const raw: unknown = new ReadOnly();
    const readonly = raw as Counter;
    let rejected = false;
    try { const old = inc(readonly); } catch (error) { rejected = error instanceof TypeError; }
    assert(rejected, "getter-only postfix throws TypeError");
}
