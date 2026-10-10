interface Value { a?: string | null }
class Holder { a?: string | null; }
function main(): void {
    const missing: Value = {};
    const present: { a?: string | null } = { a: null };
    assert(!("a" in missing), "missing key");
    assert("a" in present, "present null key");
    assert(JSON.stringify(present) === '{"a":null}', "serialize null");
    assert(JSON.stringify(missing) === '{}', "omit absent key");
    const spread: Value = { a: "old", ...present };
    assert(spread.a === null, "present null overwrites spread");
    missing.a = null;
    assert("a" in missing, "null assignment creates key");
    assert(JSON.stringify(missing) === '{"a":null}', "serialize assigned null");
    assert(Object.keys(missing).length === 1, "enumerate present null");
    assert(Object.hasOwn(missing, "a"), "has own null");
    assert(JSON.stringify(spread) === '{"a":null}', "serialize spread null");
    const blank: { a?: string | null } = {};
    const copied = { ...blank };
    copied.a = null;
    assert("a" in copied, "write spread null");
    assert(!("a" in blank), "spread presence is independent");
    const holder = new Holder();
    assert(!("a" in holder), "absent class field");
    assert(JSON.stringify(holder) === '{}', "absent class JSON");
    holder.a = null;
    assert("a" in holder, "class write null");
    assert(JSON.stringify(holder) === '{"a":null}', "class null JSON");
    const untouched: Value = {};
    assert(Object.keys(untouched).length === 0, "enumerate absent");
    assert(!Object.hasOwn(untouched, "a"), "has own absent");
    assert(!("a" in untouched), "presence is per instance");
}
