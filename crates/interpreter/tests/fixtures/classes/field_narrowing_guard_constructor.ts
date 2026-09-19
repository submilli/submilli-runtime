class Animal { speak(): string { return "animal"; } }
class Dog extends Animal { fetch(): string { return "dog"; } }
class Base<T> { value: T | Animal | null = null; reset(x: T | Animal | null): void { this.value = x; } }
class Child<T> extends Base<T> { value: T | null = null; constructor() { super(); this.reset(new Animal()); const v = this.value; assert(v !== null, "nonnull"); } }
export function main(): void {
 let caught = false;
 try { const c = new Child<Dog>(); } catch(e) { caught = e instanceof TypeError; }
 assert(caught, "constructor narrowed generic field read validates");
}
