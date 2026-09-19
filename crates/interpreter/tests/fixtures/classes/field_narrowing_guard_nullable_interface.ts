interface Inspect { inspect(): boolean; }
class Animal { speak(): string { return "animal"; } }
class Dog extends Animal { fetch(): string { return "dog"; } }
class Base<T> { value: T | Animal | null = null; reset(x: T | Animal | null): void { this.value = x; } }
class Child<T> extends Base<T> implements Inspect { value: T | null = null; inspect(): boolean { const v = this.value; return v !== null; } }
function make<T>(): Child<T> | null { const c = new Child<T>(); c.reset(new Animal()); return c; }
export function main(): void {
 const c = make<Dog>();
 if(c !== null) {
 const inspect: Inspect = c;
 let caught = false;
 try { const value = inspect.inspect(); } catch(e) { caught = e instanceof TypeError; }
 assert(caught, "bound method class uses concrete validation");
 }
}
