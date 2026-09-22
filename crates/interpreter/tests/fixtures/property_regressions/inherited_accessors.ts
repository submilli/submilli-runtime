interface Bag { note: string; }
class Parent {
    private held: string = "initial";
    get note(): string { return this.held; }
    set note(value: string) { this.held = value; }
}
class Child extends Parent implements Bag { set note(value: string) {} }
function main(): void {
    const bag: Bag = new Child();
    assert(bag.note === "initial", "inherited getter satisfies interface");
}
