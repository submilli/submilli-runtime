// A guard binds a shadow for a *place*, and two receivers that read as places
// are not ones the narrowing engine will install a view for. Each is told to
// bind first, and the message names which refusal it is rather than guessing.
// expect-error: an element is not a place a guard can narrow — bind `arr[0]` to a `const` first
// expect-error: a variable a closure reassigns cannot hold a narrowing — bind `captured` to a `const` first

class A { f: number = 1; }
class B { f: number = 2; }
function pick(flag: boolean): A | B { return flag ? new A() : new B(); }

export function main(): string {
    const arr: (A | B)[] = [new A(), new B()];
    arr[0].f = 5;

    let captured: A | B = pick(true);
    const reassign = (): void => { captured = pick(false); };
    captured.f = 5;
    reassign();
    return "x";
}
