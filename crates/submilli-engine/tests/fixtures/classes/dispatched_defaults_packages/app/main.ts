import { Base, Derived, make, call, absent, present, write, reader } from "@test/defaults";
class Local extends Base {
    greet(name: string = "local"): string { return name; }
}
function main(): void {
    const receiver = { value: 21, read: reader() };
    assert(receiver.read() === 21, "cross-package function receiver");
    assert(make().greet() === "derived", "imported override default");
    assert(call(new Local()) === "local", "consumer override default");
    assert(new Derived().greet("explicit") === "explicit", "explicit argument");
    const value = absent();
    assert(!("a" in value), "imported absent optional");
    write(value);
    assert("a" in value, "cross-package present null write");
    assert(JSON.stringify(value) === '{"a":null}', "cross-package JSON");
    assert("a" in present(), "imported present null");
}
