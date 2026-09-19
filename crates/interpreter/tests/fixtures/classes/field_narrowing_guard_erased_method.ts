class Animal { speak(): string { return "a"; } }
class Dog extends Animal { fetch(): string { return "b"; } }
class G0<T> { v: T | Animal | null = null; poke(x: T | Animal | null): void { this.v = x; } }
class G1<T> extends G0<T> { v: T | null = null; read(): T | null { return this.v; } }
export function main(): void {
 const valid = new G1<Dog>();
 valid.poke(new Dog());
 assert(valid.read()!.fetch() === "b", "valid generic method result");
 const inherited = new Concrete();
 inherited.poke(new Animal());
 let inheritedCaught = false;
 try { const value = inherited.read(); } catch (e) { inheritedCaught = e instanceof TypeError; }
 assert(inheritedCaught, "concrete subclass retains inherited type arguments");
 const g = new G1<Dog>(); g.poke(new Animal());
 let caught = false;
 try { const value = g.read(); } catch (e) { caught = e instanceof TypeError; }
 assert(caught, "erased method must validate the narrowed field before returning it");
}

class Concrete extends G1<Dog> {}
