// expect-error: member `note` is write-only (no getter)
interface Bag { readonly note: string; }
class Parent { set note(value: string) {} }
class Child extends Parent implements Bag {}
function main(): void {}
