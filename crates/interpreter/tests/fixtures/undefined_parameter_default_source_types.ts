function definiteValue(): number { return 7; }
function optionalValue(): number | undefined { return undefined; }

function inferredDefinite(read: () => number = () => value, value = definiteValue()): number { return read(); }
function inferredMaybe(read: () => number | undefined = () => value, value = optionalValue()): number | undefined { return read(); }
function annotatedDefinite(read: () => number = () => value, value: number | undefined = definiteValue()): number { return read(); }
function annotatedMaybe(read: () => number | undefined = () => value, value: number | undefined = optionalValue()): number | undefined { return read(); }

const contextualLiteral: (read?: () => number, value?: number) => number = (read = () => value, value = 7) => read();
const contextualDefinite: (read?: () => number, value?: number) => number = (read = () => value, value = definiteValue()) => read();
const contextualMaybe: (read?: () => number | undefined, value?: number) => number | undefined = (read = () => value, value = optionalValue()) => read();
const expressionDefinite: (read?: () => number, value?: number) => number = function(read = () => value, value = definiteValue()) { return read(); };
const expressionMaybe: (read?: () => number | undefined, value?: number) => number | undefined = function(read = () => value, value = optionalValue()) { return read(); };
const annotatedArrow = (read: () => number = () => value, value: number | undefined = definiteValue()): number => read();
const annotatedExpression = function(read: () => number | undefined = () => value, value: number | undefined = optionalValue()): number | undefined { return read(); };

const chainedDefinite: (first?: () => number, read?: () => number, value?: number) => number = (first = () => read(), read = () => value, value = definiteValue()) => first();
const chainedMaybe: (first?: () => number | undefined, read?: () => number | undefined, value?: number) => number | undefined = function(first = () => read(), read = () => value, value = optionalValue()) { return first(); };
const nestedDefinite: (read?: () => number, value?: number) => number = (read = () => value, value = (function(innerValue: number | undefined = definiteValue()): number { function inner(): number { return innerValue; } return inner(); })()) => read();

function inferredWrite(read: () => number = () => value, write: () => void = () => { value = 9; }, value = definiteValue()): number { write(); return read(); }
function inferredMaybeWrite(read: () => number | undefined = () => value, write: () => void = () => { value = undefined; }, value = optionalValue()): number | undefined { write(); return read(); }
function annotatedWrite(read: () => number | undefined = () => value, write: () => void = () => { value = undefined; }, value: number | undefined = definiteValue()): number | undefined { write(); return read(); }
const contextualWrite: (read?: () => number | undefined, write?: () => void, value?: number) => number | undefined = (read = () => value, write = () => { value = undefined; }, value = definiteValue()) => { write(); return read(); };
const expressionWrite: (read?: () => number | undefined, write?: () => void, value?: number) => number | undefined = function(read = () => value, write = () => { value = undefined; }, value = optionalValue()) { write(); return read(); };
function annotatedBodyWrite(read: () => number | undefined = () => value, value: number | undefined = definiteValue()): number | undefined { value = undefined; return read(); }
const contextualBodyWrite: (read?: () => number | undefined, value?: number) => number | undefined = (read = () => value, value = definiteValue()) => { value = undefined; return read(); };

function main(): void {
  assert(inferredDefinite() === 7 && inferredDefinite(undefined, 9) === 9);
  assert(inferredMaybe() === undefined && inferredMaybe(undefined, 9) === 9);
  assert(annotatedDefinite() === 7 && annotatedDefinite(undefined, 9) === 9);
  assert(annotatedMaybe() === undefined && annotatedMaybe(undefined, 9) === 9);
  assert(contextualLiteral() === 7 && contextualLiteral(undefined, 9) === 9);
  assert(contextualDefinite() === 7 && contextualDefinite(undefined, 9) === 9);
  assert(contextualMaybe() === undefined && contextualMaybe(undefined, 9) === 9);
  assert(expressionDefinite() === 7 && expressionDefinite(undefined, 9) === 9);
  assert(expressionMaybe() === undefined && expressionMaybe(undefined, 9) === 9);
  assert(annotatedArrow() === 7 && annotatedExpression() === undefined);
  assert(chainedDefinite() === 7 && chainedDefinite(undefined, undefined, 9) === 9);
  assert(chainedMaybe() === undefined && chainedMaybe(undefined, undefined, 9) === 9);
  assert(nestedDefinite() === 7 && nestedDefinite(undefined, 9) === 9, "nested default inference preserves its local functions");
  assert(inferredWrite() === 9 && inferredMaybeWrite(undefined, undefined, 9) === undefined);
  assert(annotatedWrite() === undefined && contextualWrite() === undefined && expressionWrite(undefined, undefined, 9) === undefined);
  assert(annotatedBodyWrite() === undefined && contextualBodyWrite() === undefined, "earlier closures observe later writes");
}
