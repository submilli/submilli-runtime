/** Base animal. */
export class Animal {
  /** Speak. */
  speak(): string { return "animal"; }
}
/** Dog. */
export class Dog extends Animal {
  /** Fetch. */
  fetch(): string { return "stick"; }
}
class Parent { value: Animal | null = null; reset(): void { this.value = new Animal(); } }
class Child extends Parent { value: Dog | null = new Dog(); }
/** Public view of a hidden implementation. */
export interface View {
  /** Current dog. */
  value: Dog | null;
  /** Write through the parent declaration. */
  reset(): void;
}
/** Create the hidden implementation. */
export function make(): View { return new Child(); }

/** Generic ancestor. */
export class GenericBase<T> {
  /** Current value. */
  value: T | Animal | null = null;
  /** Replace through the broad declaration. */
  reset(value: T | Animal | null): void { this.value = value; }
}
/** Generic narrowed implementation. */
export class GenericChild<T> extends GenericBase<T> {
  /** Narrowed value. */
  value: T | null = null;
  /** Read through the erased body. */
  read(): T | null { return this.value; }
}
/** Construct and read inside an erased package factory. */
function makeGeneric<T>(value: T | Animal | null): GenericChild<T> {
  const child = new GenericChild<T>();
  child.reset(value);
  const read = child.read();
  return child;
}
export { makeGeneric as makeChecked };

/** Generic factory exposed through a class. */
export class Factory {
  /** Construct and validate before returning. */
  static make<T>(value: T | Animal | null): GenericChild<T> {
    return makeGeneric<T>(value);
  }
}
