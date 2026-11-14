// A function whose signature satisfies a call-signature interface (including
// prelude constructor interfaces such as `BooleanConstructor` and
// `BigIntConstructor`) can be held, passed, returned, reassigned, inferred,
// narrowed, spread, cast, stringified, and called through that interface type.
function never(_v: unknown): boolean {
  return false;
}

const B: BooleanConstructor = never;
const BBuiltin: BooleanConstructor = Boolean;

function asParam(f: BooleanConstructor): boolean {
  return f(1);
}

function asResult(): BooleanConstructor {
  return never;
}

function id<T>(v: T): T {
  return v;
}

function run<T extends BooleanConstructor>(f: T): boolean {
  return f(1);
}

function up<U, T extends U>(f: T, _witness: U): U {
  return f;
}

function pick(): (v: unknown) => boolean {
  return never;
}

type EmptyTuple = Parameters<() => void>;

interface Predicate<T> {
  (v: T): boolean;
}

interface CombineNumbers {
  (a?: number, b?: number): number;
}

interface VariadicSum {
  (...xs: number[]): number;
}

interface Thunk {
  (): number;
}

interface PairFn {
  (a: number, b: number): number;
}

interface NumUnary {
  (n: number): number;
}

interface Callable<T> {
  (): T;
}

interface Producer<T> {
  (): T;
}

interface LabeledCallable {
  (n: number): number;
  label: string;
}

interface WeakMeta {
  tag?: string;
}

interface TaggedAction extends WeakMeta {
  (n: number): number;
}

interface WeakBox<T> {
  value?: T;
}

interface CallableWeakBox<T> extends WeakBox<T> {
  (v: T): T;
}

interface WeakNode {
  tag?: string;
  next?: WeakNode;
}

interface CallableNode extends WeakNode {
  (n: number): number;
}

interface OptionalQuotedCall {
  "@call"?(s: string): boolean;
}

class BaseWeakBox<T> {
  box: unknown = {};
}

class SubWeakBox<T> extends BaseWeakBox<T> {
  box: WeakBox<T> = {};
}

class BaseWeakNode {
  node: unknown = {};
}

class SubWeakNode extends BaseWeakNode {
  node: WeakNode = {};
}

interface Empty {}

function checkWith<T>(p: Predicate<T>, v: T): boolean {
  return p(v);
}

function callOptional(f: BooleanConstructor | undefined): boolean | undefined {
  return f?.(1);
}

function callOptionalPred(p: Predicate<number> | undefined, v: number): boolean | undefined {
  return p?.(v);
}

function makeFn<R>(r: R): () => R {
  return () => r;
}

// A value of a call-signature interface is always a function, as in
// TypeScript: `typeof` answers "function" for it, never "object".
function narrowBoolOrString(x: BooleanConstructor | string): string {
  if (typeof x === "function") {
    return x(1) ? "fn-true" : "fn-false";
  }
  return x.toUpperCase();
}

function narrowPredOrNull(x: Predicate<number> | null): string {
  if (typeof x === "object") {
    return "null";
  }
  return x(5) ? "fn-pos" : "fn-neg";
}

function classifyBySwitch(x: BooleanConstructor | string | number): string {
  switch (typeof x) {
    case "function":
      return x(1) ? "fn:t" : "fn:f";
    case "string":
      return `str:${x}`;
    case "number":
      return `num:${x}`;
    default:
      return "other";
  }
}

class BaseHolder {
  fn: unknown = never;
  variadic: unknown = (...xs: number[]): number => xs.length;
  meta: unknown = {};
}

class SubHolder extends BaseHolder {
  fn: BooleanConstructor = never;
  variadic: VariadicSum = (...xs: number[]): number => xs.length;
  meta: WeakMeta = {};
}

function zeroTop(): number {
  return 77;
}

function zeroGenericFixed<T>(): T | undefined {
  return undefined;
}

class ZeroParent {
  zero(): number {
    return 11;
  }
}

class ZeroChild extends ZeroParent {
  log: string[];
  constructor(mkEmpty: () => EmptyTuple, log: string[]) {
    super(...mkEmpty());
    this.log = log;
  }
  run(mkEmpty: () => EmptyTuple): number {
    this.log.push("before-super-method");
    return super.zero(...mkEmpty());
  }
}

function runBoxedInEmptySpread(thunk: Thunk): bigint {
  let capturedBig: bigint = 10n;
  const bump = (): EmptyTuple => {
    capturedBig += 5n;
    return [];
  };
  thunk(...bump());
  zeroTop(...bump());
  return capturedBig;
}

class CallableBox<T> {
  fn: Callable<T>;
  optFn: Callable<T> | undefined;
  constructor(fn: Callable<T>) {
    this.fn = fn;
    this.optFn = fn;
  }
}

class CounterHolder {
  base: number = 10;
  fn: NumUnary;
  optFn: NumUnary | undefined;
  constructor(fn: NumUnary) {
    this.fn = fn;
    this.optFn = fn;
  }
}

type GrowingWeakChain<T> = {
  box: WeakBox<T>;
  next: GrowingWeakChain<T[]> | null;
};

class GrowingWeakParent {
  chain: unknown = null;
  reset(v: unknown): void {
    this.chain = v;
  }
}

class GrowingWeakChild extends GrowingWeakParent {
  chain: GrowingWeakChain<number> = { box: {}, next: null };
}

type GrowingCallChain<UniqueCallElem> = {
  box: Callable<UniqueCallElem>;
  next: GrowingCallChain<UniqueCallElem[]> | null;
};

class GrowingCallParent {
  chain: unknown = null;
  reset(v: unknown): void {
    this.chain = v;
  }
}

class GrowingCallChild0 extends GrowingCallParent {
  chain: GrowingCallChain<number> = { box: () => 1, next: null };
}

class GrowingCallChild1 extends GrowingCallParent {
  chain: GrowingCallChain<number[]> = { box: () => [1], next: null };
}

interface PairTaggedCallable<A, B> {
  (a: A, b: B): A;
  first: A;
  second: B;
}

type GrowingPairCallChain<UniquePairElem> = {
  pair: PairTaggedCallable<UniquePairElem, UniquePairElem>;
  next: GrowingPairCallChain<UniquePairElem[]> | null;
};

class GrowingPairParent {
  chain: unknown = null;
  reset(v: unknown): void {
    this.chain = v;
  }
}

class GrowingPairChild extends GrowingPairParent {
  chain: GrowingPairCallChain<number> | null = null;
}

class HeteroPairChild extends GrowingPairParent {
  chain: PairTaggedCallable<string, number> | null = null;
}

interface MutualCallA<UniqueMutA> {
  fn: Callable<UniqueMutA>;
  b: MutualCallB<UniqueMutA[]> | null;
}

interface MutualCallB<UniqueMutB> {
  fn: Callable<UniqueMutB>;
  a: MutualCallA<UniqueMutB[]> | null;
}

class MutualCallParent {
  node: unknown = null;
  reset(v: unknown): void {
    this.node = v;
  }
}

class MutualCallChild extends MutualCallParent {
  node: MutualCallA<number> = { fn: () => 1, b: null };
}

class SeedBox<UniqueSeedBox> {
  value: UniqueSeedBox;
  fn: Callable<UniqueSeedBox>;
  chain: SeedBoxChain<UniqueSeedBox> | null = null;
  constructor(value: UniqueSeedBox, fn: Callable<UniqueSeedBox>) {
    this.value = value;
    this.fn = fn;
  }
}

type SeedBoxChain<UniqueSeedElem> = {
  selfNext: SeedBoxChain<UniqueSeedElem[]> | null;
  box: SeedBox<UniqueSeedElem> | null;
};

class SeedBoxParent {
  item: unknown = null;
  reset(v: unknown): void {
    this.item = v;
  }
}

class SeedBoxChild0 extends SeedBoxParent {
  item: SeedBox<string> | null = null;
}

class SeedBoxChainChild extends SeedBoxParent {
  item: SeedBoxChain<string> | null = null;
}

function main(): void {
  // Direct binding fast path vs interface-typed places.
  assert(Boolean(1) === true && Boolean(0) === false && Boolean() === false);
  assert(B(1) === false && B() === false);
  assert(BBuiltin(1) === true && BBuiltin(0) === false && BBuiltin() === false);

  // Value identity, string conversion, typeof, and JSON serialization.
  assert(String(B) === String(never) && String(B) === "[object Function]");
  assert(`${B}` === "[object Function]");
  assert(String(BBuiltin) === "function Boolean() { [native code] }");
  assert(typeof B === "function" && typeof BBuiltin === "function");
  assert(B === never && B !== Boolean && BBuiltin === Boolean);
  assert(JSON.stringify(B) === undefined && JSON.stringify(BBuiltin) === undefined);
  assert(JSON.stringify([B]) === "[null]" && JSON.stringify({ b: B, x: 1 }) === '{"x":1}');

  // Weak super-interface holding a function: typeof and JSON.stringify.
  const tagged: TaggedAction = (n) => n * 2;
  const taggedJson: string | undefined = JSON.stringify(tagged);
  assert(typeof tagged === "function" && taggedJson === undefined && tagged(6) === 12);

  // Every place where the value moves.
  assert(asParam(never) === false && asParam(Boolean) === true);
  assert(asResult()(1) === false);

  let mutable = Boolean;
  assert(mutable(1) === true);
  mutable = never;
  assert(mutable(1) === false && String(mutable) === "[object Function]");

  assert(id<BooleanConstructor>(never)(1) === false);
  assert(id<BooleanConstructor>(Boolean)(1) === true);

  assert(run(never) === false && run(Boolean) === true);
  assert(up(never, Boolean)(1) === false);

  const made: () => BooleanConstructor = pick;
  assert(made()(1) === false);

  const joined = false ? Boolean : never;
  assert(joined(1) === false);
  const pair = [Boolean, never];
  assert(pair[0]!(1) === true && pair[1]!(1) === false);

  // Contextual typing (including union contextual typing) of arrow and function expressions.
  const arrowB: BooleanConstructor = (v) => v === "secret";
  assert(!arrowB(1) && arrowB("secret"));
  const fnExprB: BooleanConstructor = function (v) {
    return v === 42;
  };
  assert(!fnExprB(1) && fnExprB(42));
  const unionFnExpr: BooleanConstructor | null = function (v) {
    return v === "ok";
  };
  const maybeUnionFn = (present: boolean): BooleanConstructor | null =>
    present ? unionFnExpr : null;
  assert(
    maybeUnionFn(true)?.("ok") === true &&
      maybeUnionFn(true)?.("no") === false &&
      maybeUnionFn(false)?.("ok") === undefined,
  );
  const unionArrowPred: Predicate<string> | number = (s) => s.length > 2;
  assert(typeof unionArrowPred === "function" && unionArrowPred("abc") === true);

  // Generic return-type seeding from a call-signature interface expectation.
  const seeded: Producer<number> = makeFn(42);
  const seededNullable: Producer<number> | null = makeFn(7);
  const maybeSeeded = (present: boolean): Producer<number> | null =>
    present ? seededNullable : null;
  assert(seeded() === 42 && maybeSeeded(true)?.() === 7 && maybeSeeded(false)?.() === undefined);

  // Symmetric comparability between function types and call-signature interfaces.
  const unary: (x: unknown) => boolean = never;
  const binary = (a: unknown, b: unknown): boolean => a === b;
  const effect = (_x: unknown): void => {};
  assert(B === unary && unary === B);
  assert(B !== binary && binary !== B);
  assert(B !== effect && effect !== B);

  // Default parameters and rest parameters through call-signature interfaces.
  const withDefault: BooleanConstructor = (v: unknown = "omitted"): boolean => v === "omitted";
  assert(withDefault() === true && withDefault(undefined) === true && withDefault(1) === false);
  const countArgs: CombineNumbers = (...xs: (number | undefined)[]) => xs.length;
  assert(countArgs() === 0 && countArgs(10) === 1 && countArgs(10, 20) === 2);

  // Arity-adapted closure unwrapped when invoked through a call-signature interface.
  const takesDefault = (v: unknown = "omitted"): boolean => v === 1;
  const zeroArity: () => boolean = takesDefault;
  const adaptedB: BooleanConstructor = zeroArity;
  assert(adaptedB(1) === true && adaptedB(2) === false && adaptedB() === false);

  // Spread arguments through direct and optional call signatures, including 0-param empty tuple and capture.
  const sum: VariadicSum = (...xs: number[]) => xs.reduce((acc, x) => acc + x, 0);
  const maybeSum = (present: boolean): VariadicSum | undefined => (present ? sum : undefined);
  const nums = [10, 20, 12];
  assert(
    sum(...nums) === 42 &&
      maybeSum(true)?.(...nums) === 42 &&
      maybeSum(false)?.(...nums) === undefined,
  );
  assert(sum(...new Set([10, 20, 12])) === 42);
  const makeSet = (...items: number[]): Set<number> => new Set(items);
  assert(sum(...makeSet(...nums)) === 42);

  let emptyCalls = 0;
  const emptyTuple = (): EmptyTuple => {
    emptyCalls += 1;
    return ([] as unknown) as EmptyTuple;
  };
  assert(BBuiltin(...emptyTuple()) === false && emptyCalls === 1);
  const thunk: Thunk = () => 99;
  const maybeThunk = (present: boolean): Thunk | undefined => (present ? thunk : undefined);
  assert(
    thunk(...emptyTuple()) === 99 &&
      maybeThunk(true)?.(...emptyTuple()) === 99 &&
      maybeThunk(false)?.(...emptyTuple()) === undefined &&
      emptyCalls === 3,
  );

  let pairCalls = 0;
  const makePair = (): [number, number] => {
    pairCalls += 1;
    return [19, 23];
  };
  const addPair: PairFn = (a, b) => a + b;
  assert(addPair(...makePair()) === 42 && pairCalls === 1);

  const capturedEmpty: EmptyTuple = ([] as unknown) as EmptyTuple;
  const callCapturedSpread = (): boolean => BBuiltin(...capturedEmpty);
  assert(callCapturedSpread() === false);

  // Object spread copies no call signature; spreading a closure yields `{}` at runtime, and
  // spreading a hybrid yields only its properties.
  const spreadPred = { ...(B as Predicate<unknown>) };
  assert(Object.keys(spreadPred).length === 0 && !("@call" in spreadPred));
  const labeledObj: LabeledCallable = Object.assign((n: number): number => n + 1, {
    label: "kept",
  });
  const spreadLabeled: { label: string } = { ...labeledObj };
  assert(spreadLabeled.label === "kept" && labeledObj(1) === 2);

  // Receiver (`this`) binding across member, optional-chain, element, detached, and hybrid calls.
  const inspectThis: NumUnary = function (this: unknown, x: number): number {
    const self = this;
    if (self instanceof CounterHolder) {
      return self.base + x;
    }
    if (Array.isArray(self)) {
      return 100 + self.length + x;
    }
    return self === undefined ? -x : -999;
  };
  const holder = new CounterHolder(inspectThis);
  const getHolder = (present: boolean): CounterHolder | null => (present ? holder : null);
  const detached = holder.fn;
  const fnArray: NumUnary[] = [inspectThis];
  assert(holder.fn(5) === 15);
  assert(getHolder(true)?.fn(5) === 15 && getHolder(false)?.fn(5) === undefined);
  assert(holder.optFn?.(5) === 15);
  assert(fnArray[0]!(5) === 106);
  let detachedThrew = false;
  try {
    detached(5);
  } catch (e) {
    detachedThrew = e instanceof TypeError;
  }
  assert(detachedThrew);

  const objectWithCall: NumUnary = Object.assign((n: number): number => n + 7, { tag: "hybrid" });
  const holderForObject = new CounterHolder(objectWithCall);
  assert(
    objectWithCall(5) === 12 &&
      holderForObject.fn(5) === 12 &&
      holderForObject.optFn?.(5) === 12,
  );

  // Direct call on a closure expecting non-undefined `this` throws `TypeError`.
  const checksThis: BooleanConstructor = function (this: { ok: boolean }, _v: unknown): boolean {
    return this.ok;
  };
  let unboundThisThrew = false;
  try {
    checksThis(1);
  } catch (e) {
    unboundThisThrew = e instanceof TypeError;
  }
  assert(unboundThisThrew);

  // `typeof` narrowing and `switch (typeof x)` with closure and hybrid inhabitants.
  const objBool: BooleanConstructor = Object.assign((v?: unknown): boolean => v === 1, {
    tag: "hybrid",
  });
  assert(
    narrowBoolOrString(never) === "fn-false" &&
      narrowBoolOrString(Boolean) === "fn-true" &&
      narrowBoolOrString("hi") === "HI" &&
      narrowBoolOrString(objBool) === "fn-true",
  );
  const objPred: Predicate<number> = Object.assign((v: number): boolean => v > 0, {
    tag: "hybrid",
  });
  assert(
    narrowPredOrNull(null) === "null" &&
      narrowPredOrNull(objPred) === "fn-pos" &&
      narrowPredOrNull((v) => v < 0) === "fn-neg",
  );
  assert(
    classifyBySwitch(Boolean) === "fn:t" &&
      classifyBySwitch(never) === "fn:f" &&
      classifyBySwitch(objBool) === "fn:t" &&
      classifyBySwitch("ab") === "str:ab" &&
      classifyBySwitch(7) === "num:7",
  );

  // Union call dispatch across function types and call-signature interfaces (closure + hybrid).
  const callUnion = (u: Predicate<number> | ((n: number) => boolean), v: number): boolean => u(v);
  const callOptUnion = (
    u: Predicate<number> | ((n: number) => boolean) | undefined,
    v: number,
  ): boolean | undefined => u?.(v);
  assert(callUnion((n) => n === 3, 3) === true && callUnion(objPred, -1) === false);
  assert(
    callOptUnion((n) => n === 3, 3) === true &&
      callOptUnion(objPred, 4) === true &&
      callOptUnion(undefined, 3) === undefined,
  );

  // Optional-chain call on a nullable call-signature interface.
  assert(
    callOptional(never) === false &&
      callOptional(Boolean) === true &&
      callOptional(undefined) === undefined,
  );
  assert(callOptionalPred((n) => n > 0, 3) === true && callOptionalPred(undefined, 3) === undefined);

  // Upcasts, runtime checked casts (`as` on function types & `Empty`), adapter-wrapped rest closures,
  // and subclass narrowed-field runtime carrier checks (`InterfaceCarrier::Function`).
  assert((never as BooleanConstructor)(1) === false);
  assert((Boolean as BooleanConstructor)(1) === true);

  const rawRest = (...xs: number[]): number => xs.reduce((a, b) => a + b, 0);
  const wrappedFixed2: (a: number, b: number) => number = rawRest;
  const erasedWrappedRest: unknown = wrappedFixed2;
  assert(((erasedWrappedRest as (...xs: number[]) => number))(4, 5, 6) === 15);

  const emptyFn = ((() => 42) as unknown) as Empty;
  assert(typeof emptyFn === "function" && JSON.stringify(emptyFn) === undefined);

  const sub = new SubHolder();
  const base: BaseHolder = sub;
  assert(sub.fn(1) === false);
  base.fn = Boolean;
  assert(sub.fn(1) === true && sub.fn(0) === false);
  base.variadic = rawRest;
  assert(sub.variadic(1, 2, 3) === 6);
  base.variadic = wrappedFixed2;
  assert(sub.variadic(4, 5, 6) === 15);
  base.fn = (a: number, b: number): boolean => a === b;
  let rejectedBadArity = false;
  try {
    const _bad = sub.fn;
  } catch (e) {
    rejectedBadArity = e instanceof TypeError;
  }
  assert(rejectedBadArity);

  // Stale narrowed return through generic `Callable<T>`: throws catchable `TypeError`, never traps.
  const numBox = new CallableBox<number>(() => 123);
  assert(numBox.fn() + 1 === 124 && (numBox.optFn?.() ?? 0) + 1 === 124);
  const erasedBox: CallableBox<unknown> = numBox;
  erasedBox.fn = (): unknown => "corrupted";
  erasedBox.optFn = (): unknown => "corrupted";
  assert(Number.isNaN(numBox.fn() - 1) && Number.isNaN((numBox.optFn?.() ?? 0) - 1));
  let badReturnThrew = false;
  try {
    const _s: string = numBox.fn().toFixed(2);
  } catch (e) {
    badReturnThrew = e instanceof TypeError;
  }
  assert(badReturnThrew);
  let badOptReturnThrew = false;
  try {
    const _s: string | undefined = numBox.optFn?.().toFixed(2);
  } catch (e) {
    badOptReturnThrew = e instanceof TypeError;
  }
  assert(badOptReturnThrew);

  // Widened optional-chain step returning stale `null` into a host `InterfaceRef` (`TextEncoder`)
  // or into a non-optional call-signature step throws catchable `TypeError` instead of Wasm trapping.
  const encBox = new CallableBox<TextEncoder>(() => new TextEncoder());
  assert(encBox.optFn?.().encode("ok").length === 2);
  const erasedEncBox: CallableBox<unknown> = encBox;
  erasedEncBox.optFn = (): unknown => null;
  let badEncReturnThrew = false;
  try {
    const _bytes = encBox.optFn?.().encode("ok");
  } catch (e) {
    badEncReturnThrew = e instanceof TypeError;
  }
  assert(badEncReturnThrew);

  const rawChainHolder = { fn: (n: number): number => n + 1 };
  const pickChainHolder = (present: boolean): { fn: NumUnary } | null =>
    present ? rawChainHolder : null;
  assert(pickChainHolder(true)?.fn(4) === 5 && pickChainHolder(false)?.fn(4) === undefined);
  const erasedChainHolder: { fn: unknown } = rawChainHolder;
  erasedChainHolder.fn = null;
  let nullChainCallThrew = false;
  try {
    const _bad = pickChainHolder(true)?.fn(4);
  } catch (e) {
    nullChainCallThrew = e instanceof TypeError;
  }
  assert(nullChainCallThrew);

  // Weak super-interface (`WeakMeta`) accepts function carriers across upcast `as`, checked `as`,
  // narrowed class fields, and `===` / `!==` comparisons while still validating object fields and
  // rejecting primitives.
  const rawTaggedFn = (n: number): number => n * 3;
  const taggedFromRaw: TaggedAction = rawTaggedFn;
  const upcastWeak = tagged as WeakMeta;
  const upcastFromRaw: WeakMeta = taggedFromRaw;
  assert(typeof upcastWeak === "function" && JSON.stringify(upcastWeak) === undefined);
  assert(upcastFromRaw === rawTaggedFn && rawTaggedFn === upcastFromRaw && upcastWeak !== rawTaggedFn);
  const optQuoted: OptionalQuotedCall = {};
  assert(optQuoted !== rawTaggedFn && rawTaggedFn !== optQuoted);
  const weakAsFn = (tagged as unknown) as WeakMeta;
  assert(typeof weakAsFn === "function" && JSON.stringify(weakAsFn) === undefined);
  const weakAsObj = ({ tag: "ok" } as unknown) as WeakMeta;
  assert(weakAsObj.tag === "ok");
  let badWeakFieldThrew = false;
  try {
    const _badField = ({ tag: 123 } as unknown) as WeakMeta;
  } catch (e) {
    badWeakFieldThrew = e instanceof TypeError;
  }
  assert(badWeakFieldThrew);
  let badWeakPrimCastThrew = false;
  try {
    const _badPrim = (42 as unknown) as WeakMeta;
  } catch (e) {
    badWeakPrimCastThrew = e instanceof TypeError;
  }
  assert(badWeakPrimCastThrew);
  base.meta = tagged;
  assert(typeof sub.meta === "function" && JSON.stringify(sub.meta) === undefined);
  base.meta = 42;
  let badWeakMetaThrew = false;
  try {
    const _m = sub.meta;
  } catch (e) {
    badWeakMetaThrew = e instanceof TypeError;
  }
  assert(badWeakMetaThrew);
  base.meta = {};

  // Generic and recursive weak interfaces preserve `$Closure` carriers in generated validators.
  const fnWeakBox: CallableWeakBox<number> = (v: number): number => v + 1;
  const subWeakBox = new SubWeakBox<number>();
  const baseWeakBox: BaseWeakBox<number> = subWeakBox;
  baseWeakBox.box = fnWeakBox;
  const readWeakBox: WeakBox<number> = subWeakBox.box;
  assert(readWeakBox === fnWeakBox && typeof readWeakBox === "function");
  const nestedWeakBox = ({ box: fnWeakBox } as unknown) as { box: WeakBox<number> };
  assert(nestedWeakBox.box === fnWeakBox);
  baseWeakBox.box = { value: "wrong" };
  let badGenericWeakFieldThrew = false;
  try {
    const _bad = subWeakBox.box;
  } catch (e) {
    badGenericWeakFieldThrew = e instanceof TypeError;
  }
  assert(badGenericWeakFieldThrew);

  const fnNode: CallableNode = (n: number): number => n * 2;
  const subWeakNode = new SubWeakNode();
  const baseWeakNode: BaseWeakNode = subWeakNode;
  baseWeakNode.node = { next: { next: fnNode } };
  const recWeakOk: WeakNode = subWeakNode.node;
  assert(recWeakOk.next?.next === fnNode);
  baseWeakNode.node = { next: { next: 123 } };
  let badRecWeakThrew = false;
  try {
    const _badRec = subWeakNode.node;
  } catch (e) {
    badRecWeakThrew = e instanceof TypeError;
  }
  assert(badRecWeakThrew);

  // Rest-vs-fixed comparability (`===` / `!==` and `as`) across function types and call-signature interfaces.
  const variadicFn: (...xs: number[]) => number = (...xs: number[]): number =>
    xs.reduce((a, b) => a + b, 0);
  const asPairFromRest: PairFn = variadicFn as PairFn;
  assert(asPairFromRest(8, 9) === 17);
  assert(asPairFromRest === variadicFn && variadicFn === asPairFromRest);
  const varSum: VariadicSum = rawRest;
  assert(varSum === wrappedFixed2 && wrappedFixed2 === varSum);
  const asVariadicFromWrapped: (...xs: number[]) => number = wrappedFixed2 as (
    ...xs: number[]
  ) => number;
  assert(asVariadicFromWrapped(4, 5, 6) === 15);
  const fixedPairFn: (a: number, b: number) => number = (a, b) => a * b;
  assert(varSum !== fixedPairFn && fixedPairFn !== varSum);
  let badVariadicDowncastThrew = false;
  try {
    const _bad = fixedPairFn as (...xs: number[]) => number;
  } catch (e) {
    badVariadicDowncastThrew = e instanceof TypeError;
  }
  assert(badVariadicDowncastThrew);

  // Zero-parameter calls with empty-tuple spreads evaluate side effects in JS order and box captures.
  const order: string[] = [];
  const mkEmpty = (label: string): EmptyTuple => {
    order.push(label);
    return [];
  };
  assert(zeroTop(...mkEmpty("top")) === 77);
  const zeroClosure = (): number => {
    order.push("closure-body");
    return 88;
  };
  assert(
    (() => {
      order.push("callee");
      return zeroClosure;
    })()(...mkEmpty("closure-arg")) === 88,
  );
  const pickOptZero = (present: boolean): (() => number) | null => (present ? zeroClosure : null);
  assert(pickOptZero(true)?.(...mkEmpty("opt-hit")) === 88);
  assert(pickOptZero(false)?.(...mkEmpty("opt-miss")) === undefined);
  assert(zeroGenericFixed<number>(...mkEmpty("generic-fixed")) === undefined);
  const child = new ZeroChild(() => mkEmpty("super-ctor"), order);
  assert(child.run(() => mkEmpty("super-method")) === 11);
  assert(Math.random(...mkEmpty("namespace-zero")) >= 0);
  assert("ok".trim(...mkEmpty("host-method")) === "ok");
  assert(
    JSON.stringify(order) ===
      '["top","callee","closure-arg","closure-body","opt-hit","closure-body","generic-fixed","super-ctor","before-super-method","super-method","namespace-zero","host-method"]',
  );
  assert(runBoxedInEmptySpread(() => 0) === 20n);

  // Widened prelude optional-chain method/property calls (`getArr()?.at(0)`,
  // `getArr()?.at(0)?.toFixed(1)`, `getArr()?.push(...new Set([10, 20]))`, `getArr()?.length`)
  // validate live return types and materialize spreads.
  const getGoodArr: () => number[] | undefined = () => [42, 99];
  const getUndefArr: () => number[] | undefined = () => undefined;
  const getNotArr: () => number[] | undefined = (() => 123) as unknown as () =>
    | number[]
    | undefined;
  assert(getGoodArr()?.at(0) === 42 && getUndefArr()?.at(0) === undefined);
  let widenedAtThrew = false;
  try {
    getNotArr()?.at(0);
  } catch (e) {
    widenedAtThrew = e instanceof TypeError;
  }
  assert(widenedAtThrew);

  const getFloatArr: () => number[] | undefined = () => [3.5];
  const getStringArr: () => number[] | undefined = (() => ["bad"]) as unknown as () =>
    | number[]
    | undefined;
  assert(getFloatArr()?.at(0)?.toFixed(1) === "3.5");
  let widenedAtToFixedThrew = false;
  try {
    getStringArr()?.at(0)?.toFixed(1);
  } catch (e) {
    widenedAtToFixedThrew = e instanceof TypeError;
  }
  assert(widenedAtToFixedThrew);

  const targetArr = [5];
  const getTargetArr: () => number[] | undefined = () => targetArr;
  assert(
    getTargetArr()?.push(...new Set([10, 20])) === 3 && JSON.stringify(targetArr) === "[5,10,20]",
  );
  let widenedPushSetThrew = false;
  try {
    getNotArr()?.push(...new Set([10, 20]));
  } catch (e) {
    widenedPushSetThrew = e instanceof TypeError;
  }
  assert(widenedPushSetThrew);

  const getPairArr: () => number[] | undefined = () => [7, 8];
  assert(getPairArr()?.length === 2 && getUndefArr()?.length === undefined);

  // `BigIntConstructor` and user generic call-signature interfaces.
  const BI: BigIntConstructor = (_v: bigint | boolean | number | string): bigint => 99n;
  assert(BI(1) === 99n && BI("2") === 99n && BigInt(1) === 1n);

  assert(!checkWith((n: number) => n > 10, 5) && checkWith((n: number) => n < 10, 5));
  assert(checkWith(Boolean, 5) && !checkWith(Boolean, 0));
  const pred: Predicate<number> = (n) => n > 0;
  assert(pred(3) && !pred(-1) && JSON.stringify(pred) === undefined);

  // Polymorphic recursive alias containing a weak interface with callable sub-interface values.
  const callableBox0: CallableWeakBox<number> = (v: number) => v + 1;
  const callableBox1: CallableWeakBox<number[]> = (vs: number[]) => vs;
  const callableBox2: CallableWeakBox<number[][]> = (vss: number[][]) => vss;
  const growingWeak = new GrowingWeakChild();
  growingWeak.reset({
    box: callableBox0,
    next: { box: callableBox1, next: { box: callableBox2, next: null } },
  });
  assert(
    typeof growingWeak.chain.box === "function" &&
      growingWeak.chain.next !== null &&
      typeof growingWeak.chain.next.box === "function" &&
      growingWeak.chain.next.next !== null &&
      typeof growingWeak.chain.next.next.box === "function",
  );

  // Multiple roots into a polymorphic recursive alias with a call-signature interface
  // preserve full runtime descriptor discovery at each root's depth.
  const callChain0 = new GrowingCallChild0();
  assert(callChain0.chain.box() === 1);
  const callChain1 = new GrowingCallChild1();
  callChain1.reset({
    box: (): number[] => [1],
    next: { box: (): number[][] => [[2]], next: null },
  });
  assert(callChain1.chain.next !== null && callChain1.chain.next.box()[0]![0] === 2);
  callChain1.reset({ box: (): number[] => [1], next: { box: 12345, next: null } });
  let growingCallCaught = false;
  try {
    const invalid = callChain1.chain;
    assert(invalid.box().length >= 0);
  } catch (e) {
    growingCallCaught = e instanceof TypeError;
  }
  assert(growingCallCaught);

  // Repeated type-parameter instantiation inside a polymorphic recursive alias coexists
  // with distinct concrete instantiations of the same multi-parameter callable interface.
  const pairObj0: PairTaggedCallable<number, number> = Object.assign((a: number, _b: number): number => a, {
    first: 10,
    second: 20,
  });
  const pairObj1: PairTaggedCallable<number[], number[]> = Object.assign((a: number[], _b: number[]): number[] => a, {
    first: [1],
    second: [2],
  });
  const pairObj2: PairTaggedCallable<number[][], number[][]> = Object.assign((a: number[][], _b: number[][]): number[][] => a, {
    first: [[3]],
    second: [[4]],
  });
  const pairChild = new GrowingPairChild();
  pairChild.reset({
    pair: pairObj0,
    next: { pair: pairObj1, next: { pair: pairObj2, next: null } },
  });
  assert(
    pairChild.chain !== null &&
      pairChild.chain.next !== null &&
      pairChild.chain.next.next !== null &&
      pairChild.chain.next.next.pair([[9]], [[8]])[0]![0] === 9,
  );
  const heteroChild = new HeteroPairChild();
  heteroChild.reset(Object.assign((a: string, _b: number): string => a, { first: "ok", second: 42 }));
  assert(heteroChild.chain !== null && heteroChild.chain("hi", 1) === "hi");
  heteroChild.reset(
    Object.assign((a: string, _b: number): string => a, {
      first: "ok",
      second: "wrong-not-number",
    }),
  );
  let heteroPairCaught = false;
  try {
    const invalidPair = heteroChild.chain;
    assert(invalidPair === null || invalidPair("x", 1).length >= 0);
  } catch (e) {
    heteroPairCaught = e instanceof TypeError;
  }
  assert(heteroPairCaught);

  // Mutually recursive generic interfaces preserve callable descriptors on both sides.
  const mutualChild = new MutualCallChild();
  mutualChild.reset({
    fn: (): number => 7,
    b: {
      fn: (): number[] => [8],
      a: { fn: (): number[][] => [[9]], b: null },
    },
  });
  assert(
    mutualChild.node.b !== null &&
      mutualChild.node.b.a !== null &&
      mutualChild.node.b.a.fn()[0]![0] === 9,
  );
  mutualChild.reset({
    fn: (): number => 7,
    b: { fn: "not-a-function", a: null },
  });
  let mutualCaught = false;
  try {
    const invalidMutual = mutualChild.node;
    assert(invalidMutual.fn() === 7);
  } catch (e) {
    mutualCaught = e instanceof TypeError;
  }
  assert(mutualCaught);

  // Walking `SeedBox<string>` first (`SeedBox` active while discovering `SeedBoxChain`)
  // must not poison `SeedBox<TypeVar>` when `SeedBoxChain<string>` is checked directly.
  const seedBoxChild = new SeedBoxChild0();
  seedBoxChild.reset(new SeedBox("seed", () => "seed"));
  assert(seedBoxChild.item !== null && seedBoxChild.item.fn() === "seed");
  const seedChainChild = new SeedBoxChainChild();
  seedChainChild.reset({
    selfNext: {
      selfNext: {
        selfNext: null,
        box: new SeedBox([["deep"]], () => [["deep"]]),
      },
      box: new SeedBox(["mid"], () => ["mid"]),
    },
    box: new SeedBox("top", () => "top"),
  });
  assert(
    seedChainChild.item !== null &&
      seedChainChild.item.selfNext !== null &&
      seedChainChild.item.selfNext.selfNext !== null &&
      seedChainChild.item.selfNext.selfNext.box !== null &&
      seedChainChild.item.selfNext.selfNext.box.fn()[0]![0] === "deep",
  );
  seedChainChild.reset({
    selfNext: {
      selfNext: {
        selfNext: null,
        box: { value: [["wrong"]], fn: () => [["wrong"]], chain: null },
      },
      box: new SeedBox(["mid"], () => ["mid"]),
    },
    box: new SeedBox("top", () => "top"),
  });
  let invalidSeedBoxCaught = false;
  try {
    const invalidChain = seedChainChild.item;
    assert(invalidChain === null || invalidChain.box === null || invalidChain.box.fn() === "top");
  } catch (e) {
    invalidSeedBoxCaught = e instanceof TypeError;
  }
  assert(invalidSeedBoxCaught);
}
