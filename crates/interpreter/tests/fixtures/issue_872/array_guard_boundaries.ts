class Parent { value: number[] | string | null = null; }
class Child extends Parent { value: number[] = []; }
class MixedParent { value: number[] | string[] = [1]; }
class MixedChild extends MixedParent { value: number[] = []; }
class UnknownParent { value: unknown = null; }
class UnknownChild extends UnknownParent { value: number[] = []; }
function main(): void {
  const child = new Child();
  const parent: Parent = child;
  parent.value = [2, 3];
  assert(child.value[1] === 3, 'ancestor-admitted array accepted');
  parent.value = 'wrong';
  let caught = false;
  try { child.value.length; } catch (error: Error) { caught = error instanceof TypeError; }
  assert(caught, 'shallow array guard rejects string');
  parent.value = null;
  caught = false;
  try { child.value.length; } catch (error: Error) { caught = error instanceof TypeError; }
  assert(caught, 'shallow array guard rejects null');
  const mixed = new MixedChild();
  const mixedParent: MixedParent = mixed;
  mixedParent.value = ['wrong'];
  caught = false;
  try { mixed.value.length; } catch (error: Error) { caught = error instanceof TypeError; }
  assert(caught, 'overlapping array alternatives retain element validation');
  const unknown = new UnknownChild();
  const unknownParent: UnknownParent = unknown;
  unknownParent.value = ['wrong'];
  caught = false;
  try { unknown.value.length; } catch (error: Error) { caught = error instanceof TypeError; }
  assert(caught, 'unknown ancestor retains element validation');
}
