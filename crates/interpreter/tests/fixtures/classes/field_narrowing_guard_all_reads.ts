class Animal {
  speak(): string { return "animal"; }
}

class Dog extends Animal {
  fetch(): string { return "stick"; }
}

class Poodle extends Dog {
  groom(): string { return "trim"; }
}

interface Holder {
  value: Dog | null;
}

interface BroadHolder {
  value: Animal | null;
}

class Parent {
  value: Animal | null = null;
  reset(): void { this.value = new Animal(); }
}

class Child extends Parent {
  value: Dog | null = new Dog();
}

class Grandchild extends Child {
  value: Poodle | null = new Poodle();
}

interface Plain {
  n: number;
}

interface WithMethod {
  n: number;
  go(): number;
}

class ImplementsMethod {
  n: number = 1;
  go(): number { return 2; }
}

class WrongMethodReturn {
  n: number = 1;
  go(): string { return "wrong"; }
}

class WrongGetterReturn {
  get n(): string { return "wrong"; }
  go(): number { return 2; }
}

class ShapeParent {
  value: Plain | null = null;
  reset(): void { this.value = { n: 5 }; }
  resetWrongMethod(): void {
    const wrong = { n: 5, go: (x: number): number => x };
    this.value = wrong;
  }
  resetWrongMethodReturn(): void { this.value = new WrongMethodReturn(); }
}

class ShapeChild extends ShapeParent {
  value: WithMethod | null = new ImplementsMethod();
}

interface NestedValue {
  n: number;
}

interface HasNested {
  child: NestedValue;
}

interface WithNestedMethod {
  child: NestedValue;
  go(): number;
}

class NestedImpl {
  child: NestedValue = { n: 1 };
  go(): number { return this.child.n; }
}

class NestedParent {
  value: HasNested | null = null;
}

class NestedChild extends NestedParent {
  value: WithNestedMethod | null = new NestedImpl();
}

interface BroadNullableNested {
  child: unknown;
  go(): number;
}

interface WithNullableNested {
  child: NestedValue | null;
  go(): number;
}

class WrongNullableNested {
  child: { wrong: number } = { wrong: 1 };
  go(): number { return 1; }
}

class NullableNestedParent {
  value: BroadNullableNested = new WrongNullableNested();
  reset(): void { this.value = new WrongNullableNested(); }
}

class NullableNestedChild extends NullableNestedParent {
  value: WithNullableNested = { child: { n: 1 }, go: (): number => 1 };
}

type RecursiveNode = { n: number; next: RecursiveNode | null };

interface BroadRecursive {
  node: unknown;
  go(): number;
}

interface WithRecursive {
  node: RecursiveNode;
  go(): number;
}

class WrongRecursive {
  node: { wrong: number } = { wrong: 1 };
  go(): number { return 1; }
}

class RecursiveParent {
  value: BroadRecursive = new WrongRecursive();
  reset(): void { this.value = new WrongRecursive(); }
}

class RecursiveChild extends RecursiveParent {
  value: WithRecursive = {
    node: { n: 1, next: null },
    go: (): number => 1,
  };
}

interface Rich<T> {
  value: T;
  go(): number;
}

interface BroadRich {
  value: unknown;
  go(): number;
}

class BroadRichImpl {
  value: string = "wrong";
  go(): number { return 1; }
}

class RichParent {
  value: BroadRich | null = null;
  reset(): void { this.value = new BroadRichImpl(); }
}

class RichChild<T> extends RichParent {
  value: Rich<T> | null = null;
}

interface HasToString {
  toString(): string;
}

class ErasedParent {
  value: unknown = null;
  reset(value: unknown): void { this.value = value; }
}

class PrimitiveInterfaceChild extends ErasedParent {
  value: HasToString = "ok";
}

class PrimitiveInterfaceWrong extends ErasedParent {
  value: WithMethod = new ImplementsMethod();
}

class MapInterfaceChild extends ErasedParent {
  value: Map<string, string> = new Map<string, string>();
}

interface HasLength {
  length: number;
}

interface HasSize {
  size: number;
}

class ArrayStructuralChild extends ErasedParent {
  value: HasLength = [new Dog()];
}

class MapStructuralChild extends ErasedParent {
  value: HasSize = new Map<Dog, Dog>();
}

class SetStructuralChild extends ErasedParent {
  value: HasSize = new Set<Dog>();
}

interface StringPop {
  pop(): string | undefined;
}

class StringPopChild extends ErasedParent {
  value: StringPop = ["ok"];
}

interface DogPusher {
  push(value: Dog): number;
}

class DogPusherChild extends ErasedParent {
  value: DogPusher = [new Animal()];
}

class PlainArrayChild extends ErasedParent {
  value: Plain[] = [{ n: 1 }];
}

class PlainTupleChild extends ErasedParent {
  value: [Plain] = [{ n: 1 }];
}

class PlainObjectChild extends ErasedParent {
  value: { child: Plain } = { child: { n: 1 } };
}

class IterableInterfaceChild extends ErasedParent {
  value: Iterable<string> = new Set<string>(["ok"]);
}

class GetterInterfaceChild extends ErasedParent {
  value: WithMethod = new ImplementsMethod();
}

let interfaceParameterGetterReads: number = 0;

class GetterOnlyPlain {
  get n(): number {
    interfaceParameterGetterReads = interfaceParameterGetterReads + 1;
    return 1;
  }
}

class TextEncoderInterfaceChild extends ErasedParent {
  value: TextEncoder = new TextEncoder();
}

interface WithArgMethod {
  go(value: number): number;
}

class ArgMethodImpl {
  go(value: number): number { return value + 1; }
}

class WrongArgMethodImpl {
  go(value: string): number { return value.length; }
}

class ArgMethodInterfaceChild extends ErasedParent {
  value: WithArgMethod = new ArgMethodImpl();
}

type CycleNode = { n: number; next: CycleNode | null };

class CycleInterfaceChild extends ErasedParent {
  value: CycleNode = { n: 1, next: null };
}

class RecursiveSetChild extends ErasedParent {
  value: Set<CycleNode> = new Set<CycleNode>();
}

type GrowingNode<T> = { value: T; next: GrowingNode<T[]> | null };

class GrowingNodeChild extends ErasedParent {
  value: GrowingNode<number> = { value: 1, next: null };
}

interface GrowingInterface<T> {
  value: T;
  next: GrowingInterface<T[]> | null;
}

class GrowingInterfaceChild extends ErasedParent {
  value: GrowingInterface<number> = { value: 1, next: null };
}

interface NodeMaker {
  make(): CycleNode;
}

class NodeMakerChild extends ErasedParent {
  value: NodeMaker = { make: (): CycleNode => ({ n: 1, next: null }) };
}

type MutualA = { a: number; next: MutualB | null };
type MutualB = { b: number; next: MutualA | null };

class WrongMutualCycle {
  a: number = 1;
  next: unknown = null;
}

class MutualInterfaceChild extends ErasedParent {
  value: MutualA = { a: 1, next: { b: 2, next: null } };
}

interface MethodNode {
  n: number;
  next: MethodNode | null;
  go(): number;
}

class WrongDeepMethodNode {
  n: number = 1;
  next: unknown = null;
  go(): number { return 1; }
}

class MethodNodeInterfaceChild extends ErasedParent {
  value: MethodNode = { n: 1, next: null, go: (): number => 1 };
}

interface MutualMethodA {
  a: number;
  next: MutualMethodB | null;
  go(): number;
}

interface MutualMethodB {
  b: number;
  next: MutualMethodA | null;
  go(): number;
}

class MutualMethodAImpl {
  a: number = 1;
  next: unknown = null;
  go(): number { return 1; }
}

class MutualMethodBImpl {
  b: number = 2;
  next: unknown = null;
  go(): number { return 2; }
}

class MutualMethodChild extends ErasedParent {
  value: MutualMethodA = { a: 1, next: null, go: (): number => 1 };
}

class GenericParent<T> {
  value: T | Animal | null = null;
  reset(value: T | Animal | null): void { this.value = value; }
}

class GenericChild<T> extends GenericParent<T> {
  value: T | null = null;
}

enum Mode {
  Ready = 1,
}

class EnumParent {
  value: Mode | Animal | null = null;
  reset(): void { this.value = new Animal(); }
}

class EnumChild extends EnumParent {
  value: Mode | null = Mode.Ready;
}

function catchesTypeError(action: () => void): boolean {
  try {
    action();
    return false;
  } catch (e) {
    return e instanceof TypeError;
  }
}

export function main(): void {
  const child = new Child();
  child.reset();
  const broad: BroadHolder = child;
  const broadValue = broad.value;
  assert(broadValue !== null && broadValue.speak() === "animal", "wider interface read");
  const holder: Holder = child;
  assert(catchesTypeError(() => { const value = holder.value; }), "interface receiver");

  const grandchild = new Grandchild();
  grandchild.reset();
  const dogHolder: Holder = grandchild;
  assert(catchesTypeError(() => { const value = dogHolder.value; }), "intermediate target");

  const shape = new ShapeChild();
  const validShape = shape.value;
  assert(validShape !== null && validShape.go() === 2, "valid method interface");
  shape.reset();
  assert(catchesTypeError(() => { const value = shape.value; }), "method interface");

  const wrongMethod = new ShapeChild();
  wrongMethod.resetWrongMethod();
  assert(catchesTypeError(() => { const value = wrongMethod.value; }), "method ABI");

  const wrongMethodReturn = new ShapeChild();
  wrongMethodReturn.resetWrongMethodReturn();
  const methodValue = wrongMethodReturn.value;
  assert(methodValue !== null, "method-bearing value is present");
  if (methodValue !== null) {
    const actual: unknown = methodValue.go();
    assert(actual === "wrong", "method calls preserve their actual return value");
  }

  const wrongGetterReturn = new GetterInterfaceChild();
  wrongGetterReturn.reset(new WrongGetterReturn());
  const getterValue = wrongGetterReturn.value;
  const getterActual: unknown = getterValue.n;
  assert(getterActual === "wrong", "getter calls preserve their actual return value");

  const primitiveInterface = new PrimitiveInterfaceChild();
  assert(primitiveInterface.value.toString() === "ok", "primitive interface value");
  const wrongPrimitive = new PrimitiveInterfaceWrong();
  wrongPrimitive.reset(7);
  assert(catchesTypeError(() => { const value = wrongPrimitive.value; }), "invalid primitive interface value");

  const mapInterface = new MapInterfaceChild();
  mapInterface.value.set("a", "b");
  const wrongMap = new Map<number, number>();
  wrongMap.set(1, 2);
  mapInterface.reset(wrongMap);
  assert(catchesTypeError(() => { const value = mapInterface.value; }), "map carrier checks entries");
  mapInterface.reset(new Set<string>());
  assert(catchesTypeError(() => { const value = mapInterface.value; }), "map rejects other host object");

  const arrayStructural = new ArrayStructuralChild();
  const arrayStructuralValue = arrayStructural.value;
  const mapStructural = new MapStructuralChild();
  const mapStructuralValue = mapStructural.value;
  const setStructural = new SetStructuralChild();
  const setStructuralValue = setStructural.value;
  const stringPop = new StringPopChild();
  stringPop.reset([1]);
  assert(catchesTypeError(() => { const value = stringPop.value; }), "array carrier preserves constrained element type");

  const dogPusher = new DogPusherChild();
  dogPusher.reset([new Animal()]);
  const dogPusherValue = dogPusher.value;

  const plainArray = new PlainArrayChild();
  plainArray.reset([{ wrong: 1 }]);
  assert(catchesTypeError(() => { const value = plainArray.value; }), "array validates nested interface elements");
  const plainTuple = new PlainTupleChild();
  plainTuple.reset([{ wrong: 1 }]);
  assert(catchesTypeError(() => { const value = plainTuple.value; }), "tuple validates nested interface elements");
  const plainObject = new PlainObjectChild();
  plainObject.reset({ child: { wrong: 1 } });
  assert(catchesTypeError(() => { const value = plainObject.value; }), "object validates nested interface fields");

  const iterableInterface = new IterableInterfaceChild();
  const iterableValue = iterableInterface.value;
  iterableInterface.reset(new Set<number>([1]));
  assert(catchesTypeError(() => { const value = iterableInterface.value; }), "set carrier checks elements");
  iterableInterface.reset([1]);
  assert(catchesTypeError(() => { const value = iterableInterface.value; }), "array carrier checks elements");

  const textEncoderInterface = new TextEncoderInterfaceChild();
  assert(textEncoderInterface.value.encode("A")[0] === 65, "direct ObjectShape carrier");

  const ignorePlain = (value: Plain): number => 0;
  assert(ignorePlain(new GetterOnlyPlain()) === 0, "interface parameter accepted");
  assert(interfaceParameterGetterReads === 0, "interface parameter validation has no getter effects");

  const argMethod = new ArgMethodInterfaceChild();
  argMethod.reset(new WrongArgMethodImpl());
  assert(catchesTypeError(() => { argMethod.value.go(1); }), "method argument representation");

  const cycle = new CycleInterfaceChild();
  const cycleValue: CycleNode = { n: 1, next: null };
  cycleValue.next = cycleValue;
  cycle.reset(cycleValue);
  assert(cycle.value.next !== null && cycle.value.next.n === 1, "recursive cycle terminates");
  cycle.reset({ wrong: 1 });
  assert(catchesTypeError(() => { const value = cycle.value; }), "direct recursive alias guard");

  const recursiveSet = new RecursiveSetChild();
  recursiveSet.value.add({ n: 1, next: null });
  recursiveSet.reset(new Set<unknown>([{ wrong: 1 }]));
  assert(catchesTypeError(() => { const value = recursiveSet.value; }), "recursive collection element guard");

  const growing = new GrowingNodeChild();
  const growingTail: GrowingNode<number[]> = { value: [2], next: null };
  growing.reset({ value: 1, next: growingTail });
  assert(growing.value.next !== null && growing.value.next.value[0] === 2, "polymorphic alias tail validates");
  growing.reset({ value: 1, next: { value: "wrong", next: null } });
  assert(catchesTypeError(() => { const value = growing.value; }), "polymorphic alias tail guard");

  const growingInterface = new GrowingInterfaceChild();
  const growingInterfaceTail: GrowingInterface<number[]> = { value: [2], next: null };
  growingInterface.reset({ value: 1, next: growingInterfaceTail });
  const growingInterfaceValue = growingInterface.value;
  growingInterface.reset({ value: 1, next: { value: "wrong", next: null } });
  assert(catchesTypeError(() => { const value = growingInterface.value; }), "polymorphic interface tail guard");

  const maker = new NodeMakerChild();
  maker.reset({ make: (): unknown => ({ wrong: 1 }) });
  const made: unknown = maker.value.make();
  assert(JSON.stringify(made) === '{"wrong":1}', "recursive alias annotations preserve actual returns");

  const mutual = new MutualInterfaceChild();
  const wrongMutual = new WrongMutualCycle();
  wrongMutual.next = wrongMutual;
  mutual.reset(wrongMutual);
  assert(catchesTypeError(() => { const value = mutual.value; }), "mutual recursive types stay distinct");

  const methodNode = new MethodNodeInterfaceChild();
  const wrongMethodNode = new WrongDeepMethodNode();
  const innerMethodNode = new WrongDeepMethodNode();
  innerMethodNode.next = { wrong: 1 };
  wrongMethodNode.next = innerMethodNode;
  methodNode.reset(wrongMethodNode);
  assert(catchesTypeError(() => { const value = methodNode.value; }), "recursive method interface");

  const mutualMethod = new MutualMethodChild();
  const mutualMethodA = new MutualMethodAImpl();
  const mutualMethodB = new MutualMethodBImpl();
  const mutualMethodTail = new MutualMethodAImpl();
  mutualMethodTail.next = { wrong: 4 };
  mutualMethodB.next = mutualMethodTail;
  mutualMethodA.next = mutualMethodB;
  mutualMethod.reset(mutualMethodA);
  assert(catchesTypeError(() => { const value = mutualMethod.value; }), "mutual recursive method interfaces");

  const nested = new NestedChild();
  const nestedValue = nested.value;
  assert(nestedValue !== null && nestedValue.go() === 1, "nested interface member");

  const nullableNested = new NullableNestedChild();
  nullableNested.reset();
  assert(catchesTypeError(() => { const value = nullableNested.value; }), "nullable nested interface member");

  const recursive = new RecursiveChild();
  recursive.reset();
  assert(catchesTypeError(() => { const value = recursive.value; }), "recursive alias interface member");

  const rich = new RichChild<number>();
  rich.reset();
  assert(catchesTypeError(() => { const value = rich.value; }), "generic interface member");

  const generic = new GenericChild<Dog>();
  generic.reset(new Animal());
  assert(catchesTypeError(() => { const value = generic.value; }), "substituted generic");

  const genericInterface = new GenericChild<WithMethod>();
  genericInterface.reset(new Animal());
  assert(catchesTypeError(() => { const value = genericInterface.value; }), "substituted interface");

  const enumValue = new EnumChild();
  enumValue.reset();
  assert(catchesTypeError(() => { const value = enumValue.value; }), "enum value");
}
