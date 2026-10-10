import { Factory, makeChecked, make, GenericChild, Dog, Animal } from "guard-lib";
export function main(): void {
 const staticValid = Factory.make<Dog>(new Dog());
 assert(staticValid.read()!.fetch() === "stick", "valid generic package static factory");
 let staticCaught = false;
 try { const c = Factory.make<Dog>(new Animal()); } catch (e) { staticCaught = e instanceof TypeError; }
 assert(staticCaught, "generic package static factory retains concrete descriptors");
 const valid = makeChecked<Dog>(new Dog());
 assert(valid.read()!.fetch() === "stick", "valid generic package factory");
 let factoryCaught = false;
 try { const c = makeChecked<Dog>(new Animal()); } catch (e) { factoryCaught = e instanceof TypeError; }
 assert(factoryCaught, "generic package factory retains concrete descriptors before returning");
 const generic = new GenericChild<Dog>();
 generic.reset(new Dog());
 assert(generic.read()!.fetch() === "stick", "valid cross-package generic read");
 generic.reset(new Animal());
 let caught = false;
 try { const value = generic.read(); } catch (e) { caught = e instanceof TypeError; }
 assert(caught, "cross-package erased method uses concrete instance guard");
 const child = make(); child.reset();
 try { const value = child.value; assert(false, "hidden class guard must reject the parent value"); } catch (e) { assert(e instanceof TypeError, "hidden class guard must throw TypeError"); }
}
