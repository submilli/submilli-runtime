class Animal {}
class Dog extends Animal {}
class G0<T> { v: T | Animal | null = null; poke(x: T | Animal | null): void { this.v = x; } }
class G1<T> extends G0<T> { v: T | null = null; inspect(): boolean { const read = this.v; return true; } }
function make<T>(): G1<T> | null { return new G1<T>(); }
export function main(): void {
 const g = make<Dog>();
 if (g !== null) {
   g.poke(new Animal());
   let caught = false;
   try { g.inspect(); } catch(e) { caught = e instanceof TypeError; }
   assert(caught, "nullable generic factory must preserve narrowed read guard");
 }
}
