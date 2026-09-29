# Upstream cases left out

Every upstream case in `controlFlow`, `expressions`, `statements`, `types` that isn't in the suite, and why. A
directory (ending in `/`) is left out whole, except for any case in the suite. Written
by `../typescript-baselines/port-suite.cjs`: change its lists or the porter, not this
file.

871 entries: 550 not supported, 4 multi-file or JavaScript, 294 the port changes what it checks, 7 duplicate, 13 checks too little, 3 porter failure.

| Case | Reason | Detail |
|:-----|:-------|:-------|
| `controlFlow/assertionTypePredicates1.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `controlFlow/assertionTypePredicates2.ts` | multi-file or JavaScript |  |
| `controlFlow/controlFlowAliasing2.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `controlFlow/controlFlowCommaOperator.ts` | not supported | expected `)`, on ` if (y = "", typeof x === "string") { ` |
| `controlFlow/controlFlowDeleteOperator.ts` | not supported | `delete` |
| `controlFlow/controlFlowElementAccessNoCrash1.ts` | not supported | expected `]` to close array type, on ` commandLineArgs: TestTscCompile["commandLineArgs"]; ` |
| `controlFlow/controlFlowForInStatement.ts` | not supported | `any` is not supported, on ` let obj: any = null as unknown as (any); ` |
| `controlFlow/controlFlowForInStatement2.ts` | not supported | expected `:` after index parameter name, on ` type A = { [keywordA]: number }; ` |
| `controlFlow/controlFlowForOfStatement.ts` | not supported | expected `;` after expression, on ` for (x of obj) { ` |
| `controlFlow/controlFlowGenericTypes.ts` | the port changes what it checks | `tsc` then reports TS18047, TS2322 |
| `controlFlow/controlFlowIfStatement.ts` | the port changes what it checks | `tsc` then reports TS2695 |
| `controlFlow/controlFlowInOperator.ts` | not supported | expected `:` after index parameter name, on ` type A = { [a]: number; }; ` |
| `controlFlow/controlFlowInstanceOfGuardPrimitives.ts` | not supported | `Date` is not supported, on ` function distinguish(thing: string \| number \| Date): void { ` |
| `controlFlow/controlFlowIterationErrors.ts` | the port changes what it checks | `tsc` then reports TS2393 |
| `controlFlow/controlFlowIterationErrorsAsync.ts` | the port changes what it checks | `tsc` then reports TS2393 |
| `controlFlow/controlFlowNullishCoalesce.ts` | the port changes what it checks | `tsc` then reports TS2448 |
| `controlFlow/controlFlowOptionalChain.ts` | the port changes what it checks | `tsc` then reports TS2721, TS18047, TS2322 |
| `controlFlow/controlFlowParameter.ts` | not supported | expected field name in object pattern, on ` { [(a = "")]: b } = {} as any ` |
| `controlFlow/controlFlowSuperPropertyAccess.ts` | not supported | `protected` is not supported, on ` protected m?(): void; ` |
| `controlFlow/definiteAssignmentAssertions.ts` | the port changes what it checks | `tsc` then reports TS1039 |
| `controlFlow/definiteAssignmentAssertionsWithObjectShortHand.ts` | not supported | expected `,` or `}`, on ` const foo = { a! } ` |
| `controlFlow/dependentDestructuredVariables.ts` | the port changes what it checks | it leaves an `undefined` it can't rewrite |
| `controlFlow/dependentDestructuredVariablesFromNestedPatterns.ts` | not supported | nested destructuring is not supported, on ` const [[p1, p1Error]] = arg; ` |
| `controlFlow/neverReturningFunctions1.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `controlFlow/switchWithConstrainedTypeVariable.ts` | not supported | expected `,` or `>`, on ` function function1<T extends 'a' \| 'b'>(key: T): void { ` |
| `expressions/arrayLiterals/arrayLiterals.ts` | the port changes what it checks | `tsc` then reports TS7008, TS2451 |
| `expressions/arrayLiterals/arrayLiterals2ES5.ts` | the port changes what it checks | `tsc` then reports TS7034, TS7005 |
| `expressions/asOperator/asOpEmitParens.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `expressions/asOperator/asOperator3.ts` | not supported | `any` is not supported, on ` function tag(...x: any[]): any { return null as unknown as (any); } ` |
| `expressions/asOperator/asOperator4.ts` | multi-file or JavaScript |  |
| `expressions/asOperator/asOperatorAmbiguity.ts` | not supported | `any` is not supported, on ` let x: any = null as unknown as (any); ` |
| `expressions/asOperator/asOperatorASI.ts` | not supported | `any` is not supported, on ` function as(...args: any[]): void { } ` |
| `expressions/asOperator/asOperatorContextualType.ts` | checks too little | 2 after the port |
| `expressions/assignmentOperator/assignmentGenericLookupTypeNarrowing.ts` | not supported | expected `:` after index parameter name, on ` let mappedObject: {[K in "foo"]: null \| {x: string}} = {foo: {x: "hello"}}; ` |
| `expressions/assignmentOperator/assignmentLHSIsReference.ts` | not supported | `any` is not supported, on ` let value: any = null as unknown as (any); ` |
| `expressions/assignmentOperator/assignmentLHSIsValue.ts` | the port changes what it checks | `tsc` then reports TS2683, TS7005 |
| `expressions/assignmentOperator/assignmentTypeNarrowing.ts` | not supported | invalid assignment target, on ` [x] = [true]; ` |
| `expressions/assignmentOperator/compoundAdditionAssignmentLHSCannotBeAssigned.ts` | not supported | `as` to `E` is not yet supported: enum targets need a per-variant value check at runtime, on ` let x3: E = null as unknown as (E); ` |
| `expressions/assignmentOperator/compoundArithmeticAssignmentLHSCanBeAssigned.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `expressions/assignmentOperator/compoundAssignmentLHSIsReference.ts` | the port changes what it checks | `tsc` then reports TS7034, TS18048, TS7005 |
| `expressions/assignmentOperator/compoundAssignmentLHSIsValue.ts` | the port changes what it checks | `tsc` then reports TS2683, TS7005 |
| `expressions/binaryOperators/additionOperator/additionOperatorWithAnyAndEveryType.ts` | the port changes what it checks | `tsc` then reports TS7005 |
| `expressions/binaryOperators/additionOperator/additionOperatorWithConstrainedTypeParameter.ts` | not supported | expected `,` or `>`, on ` function sum<T extends Record<K, number>, K extends string>(n: number, v: T, ... ` |
| `expressions/binaryOperators/additionOperator/additionOperatorWithInvalidOperands.ts` | the port changes what it checks | `tsc` then reports TS7005 |
| `expressions/binaryOperators/additionOperator/additionOperatorWithNullValueAndInvalidOperator.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `expressions/binaryOperators/additionOperator/additionOperatorWithTypeParameter.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `expressions/binaryOperators/additionOperator/additionOperatorWithUndefinedValueAndInvalidOperands.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `expressions/binaryOperators/additionOperator/additionOperatorWithUndefinedValueAndValidOperator.ts` | duplicate | of `expressions/binaryOperators/additionOperator/additionOperatorWithNullValueAndValidOperator.ts` |
| `expressions/binaryOperators/arithmeticOperator/arithmeticOperatorWithAnyAndNumber.ts` | not supported | unexpected character `&`, on ` let rh1 = a & a; ` |
| `expressions/binaryOperators/arithmeticOperator/arithmeticOperatorWithEnum.ts` | not supported | unexpected character `&`, on ` let rh1 = c & a; ` |
| `expressions/binaryOperators/arithmeticOperator/arithmeticOperatorWithEnumUnion.ts` | not supported | unexpected character `&`, on ` let rh1 = c & a; ` |
| `expressions/binaryOperators/arithmeticOperator/arithmeticOperatorWithInvalidOperands.ts` | not supported | unexpected character `&`, on ` let r8a1 = a & a; //ok ` |
| `expressions/binaryOperators/arithmeticOperator/arithmeticOperatorWithNullValueAndValidOperands.ts` | not supported | unexpected character `&`, on ` let rh1 = null & a; ` |
| `expressions/binaryOperators/arithmeticOperator/arithmeticOperatorWithTypeParameter.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `expressions/binaryOperators/arithmeticOperator/arithmeticOperatorWithUndefinedValueAndInvalidOperands.ts` | duplicate | of `expressions/binaryOperators/arithmeticOperator/arithmeticOperatorWithNullValueAndInvalidOperands.ts` |
| `expressions/binaryOperators/arithmeticOperator/arithmeticOperatorWithUndefinedValueAndValidOperands.ts` | not supported | unexpected character `&`, on ` let rh1 = null & a; ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithIdenticalObjects.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithIntersectionType.ts` | not supported | intersection types |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNoRelationshipObjectsOnCallSignature.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNoRelationshipObjectsOnConstructorSignature.ts` | not supported | construct signatures |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNoRelationshipObjectsOnIndexSignature.ts` | not supported | index signatures |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNoRelationshipObjectsOnInstantiatedCallSignature.ts` | not supported | expected `:` after field name, on ` let a1: { fn<T>(x: T): T } = null as unknown as ({ fn<T>(x: T): T }); ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNoRelationshipObjectsOnInstantiatedConstructorSignature.ts` | not supported | construct signatures |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNoRelationshipObjectsOnProperty.ts` | not supported | `as` to `A1` is not yet supported: class types aren't yet supported as `as` targets, on ` let a1: A1 = null as unknown as (A1); ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNoRelationshipPrimitiveType.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNumberOperand.ts` | not supported | unknown type `Promise`, on ` const t1: number \| Promise<number> = null as unknown as (number \| Promise<num... ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNumericLiteral.ts` | not supported | unexpected character `&`, on ` type BrandedNum = number & { __numberBrand: any }; ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithOneOperandIsAny.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithOneOperandIsNull.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithOneOperandIsUndefined.ts` | the port changes what it checks | `tsc` then reports TS2304, TS2451 |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithSubtypeObjectOnConstructorSignature.ts` | not supported | construct signatures |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithSubtypeObjectOnIndexSignature.ts` | not supported | index signatures |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithSubtypeObjectOnInstantiatedCallSignature.ts` | not supported | expected `:` after field name, on ` let a1: { fn<T>(x: T): T } = null as unknown as ({ fn<T>(x: T): T }); ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithSubtypeObjectOnInstantiatedConstructorSignature.ts` | not supported | construct signatures |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithSubtypeObjectOnProperty.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithTwoOperandsAreAny.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `expressions/binaryOperators/inOperator/` | not supported | the `in` operator |
| `expressions/binaryOperators/instanceofOperator/instanceofOperatorWithAny.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `expressions/binaryOperators/instanceofOperator/instanceofOperatorWithInvalidOperands.es2015.ts` | not supported | `any` is not supported, on ` let x: any = null as unknown as (any); ` |
| `expressions/binaryOperators/instanceofOperator/instanceofOperatorWithInvalidOperands.ts` | not supported | `any` is not supported, on ` let x: any = null as unknown as (any); ` |
| `expressions/binaryOperators/instanceofOperator/instanceofOperatorWithInvalidStaticToString.ts` | not supported | expected `;` after expression, on ` declare class StaticToString { ` |
| `expressions/binaryOperators/instanceofOperator/instanceofOperatorWithLHSIsObject.ts` | not supported | `any` is not supported, on ` let x1: any = null as unknown as (any); ` |
| `expressions/binaryOperators/instanceofOperator/instanceofOperatorWithLHSIsTypeParameter.ts` | not supported | `any` is not supported, on ` let x: any = null as unknown as (any); ` |
| `expressions/binaryOperators/instanceofOperator/instanceofOperatorWithRHSHasSymbolHasInstance.ts` | not supported | `Symbol` |
| `expressions/binaryOperators/instanceofOperator/instanceofOperatorWithRHSIsSubtypeOfFunction.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `expressions/binaryOperators/logicalAndOperator/logicalAndOperatorStrictMode.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `expressions/binaryOperators/logicalOrOperator/logicalOrExpressionIsNotContextuallyTyped.ts` | not supported | parameter `a` requires a type annotation, on ` let r = a \|\| ((a) => a.toLowerCase()); ` |
| `expressions/commaOperator/` | not supported | the comma operator |
| `expressions/conditonalOperator/conditionalOperatorConditoinIsAnyType.ts` | not supported | `any` is not supported, on ` let condAny: any = null as unknown as (any); ` |
| `expressions/conditonalOperator/conditionalOperatorWithoutIdenticalBCT.ts` | not supported | `any` is not supported, on ` class X { propertyX: any; propertyX1: number; propertyX2: string }; ` |
| `expressions/contextualTyping/functionExpressionContextualTyping1.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `expressions/contextualTyping/functionExpressionContextualTyping3.ts` | not supported | `any` is not supported, on ` f((a: any) => "") ` |
| `expressions/contextualTyping/generatedContextualTyping.ts` | the port changes what it checks | `tsc` then reports TS7008, TS7010, TS2300, TS2352, TS2322 |
| `expressions/contextualTyping/getSetAccessorContextualTyping.ts` | not supported | parameter requires a type annotation, on ` set Y(y) { } ` |
| `expressions/contextualTyping/iterableContextualTyping1.ts` | not supported | parameter `s` requires a type annotation, on ` let iter: Iterable<(x: string) => number> = [s => s.length]; ` |
| `expressions/contextualTyping/objectLiteralContextualTyping.ts` | the port changes what it checks | `tsc` then reports TS2393, TS2451, TS2322, TS2353 |
| `expressions/contextualTyping/parenthesizedContexualTyping1.ts` | the port changes what it checks | `tsc` then reports TS2345, TS2322 |
| `expressions/contextualTyping/parenthesizedContexualTyping2.ts` | not supported | expected type, on ` type FuncType = (x: <T>(p: T) => T) => typeof x; ` |
| `expressions/contextualTyping/parenthesizedContexualTyping3.ts` | not supported | expected `{`, on ` function tempFun<T>(tempStrs: TemplateStringsArray, g: (x: T) => T, x: T): T; ` |
| `expressions/contextualTyping/taggedTemplateContextualTyping1.ts` | not supported | expected type, on ` type FuncType = (x: <T>(p: T) => T) => typeof x; ` |
| `expressions/contextualTyping/taggedTemplateContextualTyping2.ts` | not supported | expected type, on ` type FuncType1 = (x: <T>(p: T) => T) => typeof x; ` |
| `expressions/elementAccess/letIdentifierInElementAccess01.ts` | the port changes what it checks | `tsc` then reports TS2480 |
| `expressions/elementAccess/stringEnumInElementAccess01.ts` | not supported | `as` to `E` is not yet supported: enum targets need a per-variant value check at runtime, on ` const e: E = null as unknown as (E); ` |
| `expressions/functionCalls/callOverload.ts` | not supported | `any` is not supported, on ` function fn(x: any): void { } ` |
| `expressions/functionCalls/callWithMissingVoid.ts` | not supported | `any` is not supported, on ` const xAny: X<any> = null as unknown as (X<any>); ` |
| `expressions/functionCalls/callWithMissingVoidUndefinedUnknownAnyInJs.ts` | not supported | JavaScript |
| `expressions/functionCalls/callWithSpread2.ts` | not supported | optional function parameters are not yet supported, on ` function all(a?: number, b?: number): void { } ` |
| `expressions/functionCalls/callWithSpread3.ts` | not supported | rest elements in tuple types are not supported, on ` const s2_: [string, string, ...string[]] = null as unknown as ([string, strin... ` |
| `expressions/functionCalls/callWithSpread4.ts` | not supported | expected field name in object type, on ` (s1: R, s2: RW, s3: RW, s4: RW, s5: W): Promise<void>; ` |
| `expressions/functionCalls/callWithSpread5.ts` | not supported | optional tuple elements are not supported, on ` const nnnu: [number, number, number?] = null as unknown as ([number, number, ... ` |
| `expressions/functionCalls/callWithSpreadES6.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `expressions/functionCalls/forgottenNew.ts` | not supported | expected `;` after expression, on ` namespace Tools { ` |
| `expressions/functionCalls/functionCalls.ts` | not supported | `any` is not supported, on ` let anyVar: any = null as unknown as (any); ` |
| `expressions/functionCalls/grammarAmbiguities.ts` | the port changes what it checks | `tsc` then reports TS18048 |
| `expressions/functionCalls/newWithSpread.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `expressions/functionCalls/newWithSpreadES5.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `expressions/functionCalls/newWithSpreadES6.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `expressions/functionCalls/overloadResolution.ts` | the port changes what it checks | `tsc` then reports TS7008, TS2394, TS2451, TS7010 |
| `expressions/functionCalls/overloadResolutionClassConstructors.ts` | the port changes what it checks | `tsc` then reports TS7008, TS2322, TS2409 |
| `expressions/functionCalls/overloadResolutionConstructors.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `expressions/functionCalls/typeArgumentInference.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `expressions/functionCalls/typeArgumentInferenceConstructSignatures.ts` | not supported | construct signatures |
| `expressions/functionCalls/typeArgumentInferenceTransitiveConstraints.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `expressions/functionCalls/typeArgumentInferenceWithConstraints.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `expressions/functionCalls/typeArgumentInferenceWithObjectLiteral.ts` | the port changes what it checks | `tsc` then reports TS7010, TS2451 |
| `expressions/functions/arrowFunctionContexts.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2683, TS7006 |
| `expressions/functions/arrowFunctionExpressions.ts` | the port changes what it checks | `tsc` then reports TS2451, TS7006, TS7031, TS2683 |
| `expressions/functions/contextuallyTypedIife.ts` | the port changes what it checks | `tsc` then reports TS1359, TS18048, TS7006 |
| `expressions/functions/contextuallyTypedIifeStrict.ts` | the port changes what it checks | `tsc` then reports TS1359 |
| `expressions/functions/typeOfThisInFunctionExpression.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `expressions/functions/voidParamAssignmentCompatibility.ts` | not supported | `void` cannot be a parameter type — it has no values, on ` function g(a: void): void { } ` |
| `expressions/identifiers/scopeResolutionIdentifiers.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `expressions/literals/literals.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `expressions/literals/strictModeOctalLiterals.ts` | not supported | expected `,` or `}` after enum member, on ` A = 12 + 01 ` |
| `expressions/newOperator/newOperatorConformance.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `expressions/newOperator/newOperatorErrorCases_noImplicitAny.ts` | not supported | `this` is a reserved keyword and can't be used as a name, on ` function fnNumber(this: void): number { return 90; } ` |
| `expressions/newOperator/newOperatorErrorCases.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `expressions/nullishCoalescingOperator/nullishCoalescingAssignmentVsPrivateFieldsJsEmit1.ts` | not supported | unexpected character `#`, on ` #privateProp: number \| null; ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperator_es2020.ts` | duplicate | of `expressions/nullishCoalescingOperator/nullishCoalescingOperator2.ts` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperator_not_strict.ts` | duplicate | of `expressions/nullishCoalescingOperator/nullishCoalescingOperator2.ts` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperator12.ts` | not supported | `any` is not supported, on ` const obj: { arr: any[] } = { arr: [] }; ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperator6.ts` | not supported | parameter requires a type annotation, on ` function foo(foo: string, bar = foo ?? "bar"): void { } ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperator9.ts` | not supported | expected expression, on ` let g = f \|\| (abc => { void abc.toLowerCase() }) ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperatorInAsyncGenerator.ts` | not supported | expected `;` after expression, on ` async function* f(a: { b?: number }): AsyncGenerator<number, void, unknown> { ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperatorInParameterBindingPattern.2.ts` | the port changes what it checks | `tsc` then reports TS2537, TS2339 |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperatorInParameterBindingPattern.ts` | the port changes what it checks | `tsc` then reports TS2537 |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperatorInParameterInitializer.2.ts` | not supported | default parameter values are only supported on function declarations, on ` ((b = a() ?? "d") => { let a; })(); ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperatorInParameterInitializer.ts` | not supported | default parameter values are only supported on function declarations, on ` ((b = a() ?? "d") => {})(); ` |
| `expressions/objectLiterals/objectLiteralErrors.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `expressions/objectLiterals/objectLiteralGettersAndSetters.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2322, TS7032, TS7006 |
| `expressions/operators/incrementAndDecrement.ts` | the port changes what it checks | `tsc` then reports TS2356 |
| `expressions/optionalChaining/callChain/callChain.3.ts` | not supported | expected `:` after field name, on ` const a: { m?<T>(obj: {x: T}): T } \| null = null as unknown as ({ m?<T>(obj: ... ` |
| `expressions/optionalChaining/callChain/callChain.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `expressions/optionalChaining/callChain/callChainInference.ts` | not supported | `this` is a reserved keyword and can't be used as a name, on ` foo<T>(this: T, arg: keyof T): void; ` |
| `expressions/optionalChaining/callChain/callChainWithSuper.ts` | not supported | expected `:` and a type for the class field, on ` class Base { method?(): void {} } ` |
| `expressions/optionalChaining/callChain/parentheses.ts` | not supported | `any` is not supported, on ` const o1: ((...args: any[]) => number) = null as unknown as (((...args: any[]... ` |
| `expressions/optionalChaining/callChain/superMethodCall.ts` | not supported | expected `:` and a type for the class field, on ` method?(): void { } ` |
| `expressions/optionalChaining/callChain/thisMethodCall.ts` | not supported | expected `:` and a type for the class field, on ` method?(): void {} ` |
| `expressions/optionalChaining/delete/` | not supported | `delete` |
| `expressions/optionalChaining/elementAccessChain/elementAccessChain.3.ts` | not supported | `any` is not supported, on ` const obj: any = null as unknown as (any); ` |
| `expressions/optionalChaining/optionalChainingInference.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `expressions/optionalChaining/optionalChainingInLoop.ts` | not supported | `any` is not supported, on ` const list: any[] = [] ` |
| `expressions/optionalChaining/optionalChainingInParameterBindingPattern.2.ts` | the port changes what it checks | `tsc` then reports TS2464, TS2537, TS2538 |
| `expressions/optionalChaining/optionalChainingInParameterBindingPattern.ts` | the port changes what it checks | `tsc` then reports TS2464, TS2537, TS2538 |
| `expressions/optionalChaining/optionalChainingInParameterInitializer.2.ts` | not supported | default parameter values are only supported on function declarations, on ` ((b = a()?.d) => { let a; })(); ` |
| `expressions/optionalChaining/optionalChainingInParameterInitializer.ts` | not supported | default parameter values are only supported on function declarations, on ` ((b = a()?.d) => {})(); ` |
| `expressions/optionalChaining/optionalChainingInTypeAssertions.ts` | not supported | `any` is not supported, on ` (foo.m as any)?.(); ` |
| `expressions/optionalChaining/privateIdentifierChain/` | not supported | private `#names` |
| `expressions/optionalChaining/propertyAccessChain/propertyAccessChain.3.ts` | not supported | `any` is not supported, on ` const obj: any = null as unknown as (any); ` |
| `expressions/optionalChaining/taggedTemplateChain/` | not supported | tagged templates |
| `expressions/propertyAccess/propertyAccess.ts` | the port changes what it checks | `tsc` then reports TS2451, TS7053, TS7015 |
| `expressions/propertyAccess/propertyAccessWidening.ts` | not supported | `any` is not supported, on ` function g1(headerNames: any): void { ` |
| `expressions/superCalls/errorSuperCalls.ts` | not supported | parameter requires a type annotation, on ` set foo(v) { ` |
| `expressions/superCalls/superCalls.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `expressions/superPropertyAccess/errorSuperPropertyAccess.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `expressions/superPropertyAccess/superPropertyAccessNoError.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `expressions/superPropertyAccess/superSymbolIndexedAccess1.ts` | not supported | expected class member name, on ` [symbol](): number { ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess2.ts` | not supported | expected class member name, on ` [Symbol.isConcatSpreadable](): number { ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess3.ts` | not supported | expected class member name, on ` [symbol](): number { ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess4.ts` | not supported | expected class member name, on ` [symbol](): any { ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess5.ts` | not supported | `any` is not supported, on ` let symbol: any = null as unknown as (any); ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess6.ts` | not supported | `any` is not supported, on ` let symbol: any = null as unknown as (any); ` |
| `expressions/thisKeyword/thisInInvalidContexts.ts` | the port changes what it checks | `tsc` then reports TS7008, TS2683 |
| `expressions/thisKeyword/thisInInvalidContextsExternalModule.ts` | the port changes what it checks | `tsc` then reports TS7008, TS2683, TS1203 |
| `expressions/thisKeyword/thisInObjectLiterals.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `expressions/thisKeyword/typeOfThisGeneral.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2565, TS1263, TS2300, TS2683, TS7041, TS7017 |
| `expressions/thisKeyword/typeOfThisInConstructorParamList.ts` | not supported | parameter requires a type annotation, on ` constructor(f = this) { } ` |
| `expressions/typeAssertions/constAssertionOnEnum.ts` | multi-file or JavaScript |  |
| `expressions/typeAssertions/constAssertions.ts` | not supported | expected type, on ` let v1 = 'abc' as const; ` |
| `expressions/typeAssertions/typeAssertions.ts` | the port changes what it checks | `tsc` then reports TS2451, TS7008 |
| `expressions/typeGuards/typeGuardEnums.ts` | not supported | `as` to `number \| string \| E \| V` is not yet supported: enum targets need a per-variant value check at runtime, on ` let x: number\|string\|E\|V = null as unknown as (number\|string\|E\|V); ` |
| `expressions/typeGuards/typeGuardFunction.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `expressions/typeGuards/typeGuardFunctionErrors.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `expressions/typeGuards/typeGuardFunctionGenerics.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `expressions/typeGuards/typeGuardFunctionOfFormThis.ts` | not supported | expected type, on ` isLeader(): this is LeadGuard { ` |
| `expressions/typeGuards/typeGuardFunctionOfFormThisErrors.ts` | not supported | expected type, on ` isLeader(): this is LeadGuard { ` |
| `expressions/typeGuards/typeGuardInClass.ts` | not supported | expected expression, on ` let n = class { ` |
| `expressions/typeGuards/typeGuardIntersectionTypes.ts` | not supported | intersection types |
| `expressions/typeGuards/typeGuardNarrowsPrimitiveIntersection.ts` | not supported | intersection types |
| `expressions/typeGuards/typeGuardOfFormExpr1AndExpr2.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `expressions/typeGuards/typeGuardOfFormExpr1OrExpr2.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `expressions/typeGuards/typeGuardOfFormInstanceOf.ts` | not supported | `as` to `C1 \| C2` is not yet supported: class types aren't yet supported as `as` targets, on ` let ctor1: C1 \| C2 = null as unknown as (C1 \| C2); ` |
| `expressions/typeGuards/typeGuardOfFormIsType.ts` | not supported | `any` is not supported, on ` function isC1(x: any): x is C1 { ` |
| `expressions/typeGuards/typeGuardOfFormThisMember.ts` | not supported | unexpected character `&`, on ` isNetworked: this is (Networked & this); ` |
| `expressions/typeGuards/typeGuardOfFormThisMemberErrors.ts` | not supported | unexpected character `&`, on ` isNetworked: this is (Networked & this); ` |
| `expressions/typeGuards/typeGuardOfFormTypeOfOther.ts` | not supported | `as` to `string \| C` is not yet supported: class types aren't yet supported as `as` targets, on ` let strOrC: string \| C = null as unknown as (string \| C); ` |
| `expressions/typeGuards/typeGuardsDefeat.ts` | the port changes what it checks | `tsc` then reports TS2366, TS2304 |
| `expressions/typeGuards/typeGuardsInClassAccessors.ts` | not supported | static accessors are not supported, on ` static get s1() { ` |
| `expressions/typeGuards/typeGuardsInModule.ts` | not supported | expected `;` after expression, on ` namespace m1 { ` |
| `expressions/typeGuards/typeGuardsObjectMethods.ts` | not supported | expected `,` or `}`, on ` get prop() { ` |
| `expressions/typeGuards/typeGuardsWithAny.ts` | not supported | `any` is not supported, on ` let x: any = { p: 0 }; ` |
| `expressions/typeGuards/typeGuardsWithInstanceOf.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `expressions/typeGuards/typeGuardsWithInstanceOfByConstructorSignature.ts` | not supported | construct signatures |
| `expressions/typeGuards/typeGuardsWithInstanceOfBySymbolHasInstance.ts` | not supported | `Symbol` |
| `expressions/typeGuards/typeGuardTypeOfUndefined.ts` | not supported | `undefined` |
| `expressions/typeGuards/TypeGuardWithArrayUnion.ts` | the port changes what it checks | `tsc` then reports TS2366 |
| `expressions/typeGuards/TypeGuardWithEnumUnion.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `expressions/typeGuards/typePredicateASI.ts` | not supported | `any` is not supported, on ` foo(callback: (a: any, b: any) => void): I ` |
| `expressions/typeGuards/typePredicateOnVariableDeclaration01.ts` | not supported | expected type, on ` let x: this is string = null as unknown as (this is string); ` |
| `expressions/typeGuards/typePredicateOnVariableDeclaration02.ts` | not supported | expected `;` after declaration, on ` let y: z = null as unknown as (z) is number; ` |
| `expressions/typeSatisfaction/` | not supported | `satisfies` |
| `expressions/unaryOperators/bitwiseNotOperator/` | not supported | bitwise operators |
| `expressions/unaryOperators/decrementOperator/decrementOperatorWithAnyOtherType.ts` | the port changes what it checks | `tsc` then reports TS18047, TS2339 |
| `expressions/unaryOperators/decrementOperator/decrementOperatorWithEnumType.ts` | not supported | expected enum member name, on ` enum ENUM1 { A, B, "" }; ` |
| `expressions/unaryOperators/decrementOperator/decrementOperatorWithEnumTypeInvalidOperations.ts` | the port changes what it checks | `tsc` then reports TS7015 |
| `expressions/unaryOperators/decrementOperator/decrementOperatorWithNumberType.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `expressions/unaryOperators/decrementOperator/decrementOperatorWithUnsupportedBooleanType.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `expressions/unaryOperators/decrementOperator/decrementOperatorWithUnsupportedStringType.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `expressions/unaryOperators/deleteOperator/` | not supported | `delete` |
| `expressions/unaryOperators/incrementOperator/incrementOperatorWithAnyOtherType.ts` | the port changes what it checks | `tsc` then reports TS18047, TS2339 |
| `expressions/unaryOperators/incrementOperator/incrementOperatorWithAnyOtherTypeInvalidOperations.ts` | not supported | `any` is not supported, on ` let ANY1: any = null as unknown as (any); ` |
| `expressions/unaryOperators/incrementOperator/incrementOperatorWithEnumType.ts` | not supported | expected enum member name, on ` enum ENUM1 { A, B, "" }; ` |
| `expressions/unaryOperators/incrementOperator/incrementOperatorWithEnumTypeInvalidOperations.ts` | not supported | expected enum member name, on ` enum ENUM1 { A, B, "" }; ` |
| `expressions/unaryOperators/incrementOperator/incrementOperatorWithNumberType.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `expressions/unaryOperators/incrementOperator/incrementOperatorWithUnsupportedBooleanType.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `expressions/unaryOperators/incrementOperator/incrementOperatorWithUnsupportedStringType.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `expressions/unaryOperators/logicalNotOperator/logicalNotOperatorInvalidOperations.ts` | not supported | expected expression, on ` let BOOLEAN3 =!; ` |
| `expressions/unaryOperators/logicalNotOperator/logicalNotOperatorWithAnyOtherType.ts` | the port changes what it checks | `tsc` then reports TS2322, TS2339 |
| `expressions/unaryOperators/logicalNotOperator/logicalNotOperatorWithBooleanType.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `expressions/unaryOperators/logicalNotOperator/logicalNotOperatorWithNumberType.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `expressions/unaryOperators/logicalNotOperator/logicalNotOperatorWithStringType.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `expressions/unaryOperators/negateOperator/negateOperatorInvalidOperations.ts` | the port changes what it checks | `tsc` then reports TS2304, TS2451 |
| `expressions/unaryOperators/negateOperator/negateOperatorWithAnyOtherType.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `expressions/unaryOperators/negateOperator/negateOperatorWithBooleanType.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `expressions/unaryOperators/negateOperator/negateOperatorWithEnumType.ts` | not supported | expected enum member name, on ` enum ENUM1 { A, B, "" }; ` |
| `expressions/unaryOperators/negateOperator/negateOperatorWithNumberType.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `expressions/unaryOperators/plusOperator/plusOperatorInvalidOperations.ts` | not supported | `let` declaration requires an initializer, on ` let b; ` |
| `expressions/unaryOperators/plusOperator/plusOperatorWithAnyOtherType.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `expressions/unaryOperators/plusOperator/plusOperatorWithEnumType.ts` | not supported | expected enum member name, on ` enum ENUM1 { A, B, "" }; ` |
| `expressions/unaryOperators/typeofOperator/` | not supported | `typeof` as an expression |
| `expressions/unaryOperators/voidOperator/` | not supported | the `void` operator |
| `expressions/valuesAndReferences/assignments.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `expressions/valuesAndReferences/assignmentToParenthesizedIdentifiers.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `statements/breakStatements/doWhileBreakStatements.ts` | not supported | expected `;` after expression, on ` ONE: ` |
| `statements/breakStatements/forBreakStatements.ts` | not supported | expected `;` after expression, on ` ONE: ` |
| `statements/breakStatements/forInBreakStatements.ts` | not supported | `let` declaration requires an initializer, on ` for(let x in {}) { ` |
| `statements/breakStatements/invalidDoWhileBreakStatements.ts` | not supported | expected `;` after expression, on ` ONE: ` |
| `statements/breakStatements/invalidForBreakStatements.ts` | not supported | expected `;` after expression, on ` ONE: ` |
| `statements/breakStatements/invalidForInBreakStatements.ts` | not supported | expected `;` after expression, on ` ONE: ` |
| `statements/breakStatements/invalidWhileBreakStatements.ts` | not supported | expected `;` after expression, on ` ONE: ` |
| `statements/breakStatements/switchBreakStatements.ts` | not supported | expected `;` after expression, on ` ONE: ` |
| `statements/breakStatements/whileBreakStatements.ts` | not supported | expected `;` after expression, on ` ONE: ` |
| `statements/continueStatements/doWhileContinueStatements.ts` | not supported | expected `;` after expression, on ` ONE: ` |
| `statements/continueStatements/forContinueStatements.ts` | not supported | expected `;` after expression, on ` ONE: ` |
| `statements/continueStatements/forInContinueStatements.ts` | not supported | `let` declaration requires an initializer, on ` for(let x in {}) { ` |
| `statements/continueStatements/invalidDoWhileContinueStatements.ts` | not supported | expected `;` after expression, on ` ONE: ` |
| `statements/continueStatements/invalidForContinueStatements.ts` | not supported | expected `;` after expression, on ` ONE: ` |
| `statements/continueStatements/invalidForInContinueStatements.ts` | not supported | expected `;` after expression, on ` ONE: ` |
| `statements/continueStatements/invalidWhileContinueStatements.ts` | not supported | expected `;` after expression, on ` ONE: ` |
| `statements/continueStatements/whileContinueStatements.ts` | not supported | expected `;` after expression, on ` ONE: ` |
| `statements/for-await-ofStatements/` | not supported | async/await |
| `statements/for-inStatements/` | not supported | `for…in` |
| `statements/for-ofStatements/ES5For-of10.ts` | the port changes what it checks | `tsc` then reports TS1156 |
| `statements/for-ofStatements/ES5For-of11.ts` | not supported | `let` declaration requires an initializer, on ` let v; ` |
| `statements/for-ofStatements/ES5For-of12.ts` | not supported | expected `;` after expression, on ` for ([""] of [[""]]) { } ` |
| `statements/for-ofStatements/ES5For-of19.ts` | checks too little | 3 after the port |
| `statements/for-ofStatements/ES5For-of20.ts` | the port changes what it checks | `tsc` then reports TS7022, TS7005 |
| `statements/for-ofStatements/ES5For-of26.ts` | not supported | default values inside destructuring patterns are not supported, on ` for (let [a = 0, b = 1] of [2, 3]) { ` |
| `statements/for-ofStatements/ES5For-of27.ts` | not supported | default values inside destructuring patterns are not supported, on ` for (let {x: a = 0, y: b = 1} of [2, 3]) { ` |
| `statements/for-ofStatements/ES5For-of28.ts` | not supported | default values inside destructuring patterns are not supported, on ` for (let [a = 0, b = 1] of [2, 3]) { ` |
| `statements/for-ofStatements/ES5For-of29.ts` | not supported | default values inside destructuring patterns are not supported, on ` for (const {x: a = 0, y: b = 1} of [2, 3]) { ` |
| `statements/for-ofStatements/ES5For-of30.ts` | not supported | expected `;` after declaration, on ` let a: string = null as unknown as (string), b: number = null as unknown as (... ` |
| `statements/for-ofStatements/ES5For-of31.ts` | not supported | expected `;` after declaration, on ` let a: string = null as unknown as (string), b: number = null as unknown as (... ` |
| `statements/for-ofStatements/ES5For-of34.ts` | not supported | expected `;` after expression, on ` for (foo().x of ['a', 'b', 'c']) { ` |
| `statements/for-ofStatements/ES5For-of35.ts` | not supported | default values inside destructuring patterns are not supported, on ` for (const {x: a = 0, y: b = 1} of [2, 3]) { ` |
| `statements/for-ofStatements/ES5For-of36.ts` | not supported | default values inside destructuring patterns are not supported, on ` for (let [a = 0, b = 1] of [2, 3]) { ` |
| `statements/for-ofStatements/ES5For-of4.ts` | the port changes what it checks | `tsc` then reports TS1156, TS2304 |
| `statements/for-ofStatements/ES5For-of8.ts` | not supported | expected `;` after expression, on ` for (foo().x of ['a', 'b', 'c']) { ` |
| `statements/for-ofStatements/ES5For-of9.ts` | not supported | expected `;` after expression, on ` for (foo().x of []) { ` |
| `statements/for-ofStatements/ES5For-ofTypeCheck10.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `statements/for-ofStatements/ES5For-ofTypeCheck11.ts` | not supported | expected `;` after expression, on ` for (v of union) { } ` |
| `statements/for-ofStatements/ES5For-ofTypeCheck14.ts` | not supported | `as` to `string \| Set<number>` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let union: string \| Set<number> = null as unknown as (string \| Set<number>); ` |
| `statements/for-ofStatements/ES5For-ofTypeCheck8.ts` | not supported | expected `;` after expression, on ` for (v of union) { } ` |
| `statements/for-ofStatements/ES5For-ofTypeCheck9.ts` | not supported | unknown type `symbol`, on ` let union: string \| string[] \| number \| symbol = null as unknown as (string \|... ` |
| `statements/forStatements/forStatements.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `statements/forStatements/forStatementsMultipleValidDecl.ts` | the port changes what it checks | `tsc` then reports TS2502 |
| `statements/labeledStatements/` | not supported | labeled statements |
| `statements/returnStatements/returnStatementNoAsiAfterTransform.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `statements/switchStatements/switchStatements.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `statements/throwStatements/invalidThrowStatement.ts` | porter failure | still pruning after 40 passes |
| `statements/throwStatements/throwInEnclosingStatements.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `statements/throwStatements/throwStatements.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `statements/tryStatements/catchClauseWithTypeAnnotation.ts` | not supported | `any` is not supported, on ` type any1 = any; ` |
| `statements/tryStatements/invalidTryStatements.ts` | not supported | expected expression, on ` catch(x) { } // error missing try ` |
| `statements/tryStatements/tryStatements.ts` | the port changes what it checks | `tsc` then reports TS2492 |
| `statements/VariableStatements/everyTypeWithAnnotationAndInitializer.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `statements/VariableStatements/everyTypeWithInitializer.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `statements/VariableStatements/invalidMultipleVariableDeclarations.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `statements/VariableStatements/recursiveInitializer.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448, TS2454, TS7023 |
| `statements/VariableStatements/usingDeclarations/` | not supported | `using` declarations |
| `statements/VariableStatements/validMultipleVariableDeclarations.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `statements/withStatements/` | not supported | `with` |
| `types/any/` | not supported | `any` |
| `types/asyncGenerators/` | not supported | async/await and generators |
| `types/conditional/` | not supported | conditional types |
| `types/contextualTypes/asyncFunctions/` | not supported | async/await |
| `types/contextualTypes/commaOperator/` | not supported | the comma operator |
| `types/contextualTypes/jsdoc/` | not supported | JSDoc types in JavaScript |
| `types/contextualTypes/methodDeclarations/contextuallyTypedBindingInitializer.ts` | not supported | default values inside destructuring patterns are not supported, on ` function f({ show = v => v.toString() }: Show): void {} ` |
| `types/contextualTypes/methodDeclarations/contextuallyTypedBindingInitializerNegative.ts` | not supported | default values inside destructuring patterns are not supported, on ` function f({ show: showRename = v => v }: Show): void {} ` |
| `types/contextualTypes/methodDeclarations/contextuallyTypedClassExpressionMethodDeclaration01.ts` | not supported | expected expression, on ` return class { ` |
| `types/contextualTypes/methodDeclarations/contextuallyTypedClassExpressionMethodDeclaration02.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/contextualTypes/partiallyAnnotatedFunction/partiallyAnnotatedFunctionInferenceError.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/contextualTypes/partiallyAnnotatedFunction/partiallyAnnotatedFunctionInferenceWithTypeParameter.ts` | not supported | expected `,` or `>`, on ` function test<T extends C>(a: (t: T, t1: T) => void): T { return null as unkn... ` |
| `types/forAwait/` | not supported | async/await |
| `types/import/` | not supported | `import()` types |
| `types/intersection/` | not supported | intersection types |
| `types/keyof/circularIndexedAccessErrors.ts` | not supported | expected `]` to close array type, on ` x: T1["x"];  // Error ` |
| `types/keyof/keyofAndForIn.ts` | not supported | expected `,` or `>`, on ` function f1<K extends string, T>(obj: { [P in K]: T }, k: K): void { ` |
| `types/keyof/keyofAndIndexedAccess.ts` | the port changes what it checks | `tsc` then reports TS2345 |
| `types/keyof/keyofAndIndexedAccess2.ts` | not supported | expected `,` or `>`, on ` function f2<T extends { [key: string]: number }>(a: { x: number, y: number },... ` |
| `types/keyof/keyofAndIndexedAccessErrors.ts` | not supported | unexpected character `&`, on ` type T21 = Shape[string & number]; ` |
| `types/keyof/keyofIntersection.ts` | not supported | intersection types |
| `types/literal/booleanLiteralTypes1.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2393, TS2345 |
| `types/literal/booleanLiteralTypes2.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2393, TS2345 |
| `types/literal/enumLiteralTypes1.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2393, TS2345 |
| `types/literal/enumLiteralTypes2.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2393, TS2345 |
| `types/literal/enumLiteralTypes3.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum Choice { Unknown, Yes, No }; ` |
| `types/literal/literalTypesWidenInParameterPosition.ts` | not supported | class fields require a type annotation, on ` readonly noWiden = 1 ` |
| `types/literal/numericLiteralTypes1.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2393, TS2345 |
| `types/literal/numericLiteralTypes2.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2393, TS2345 |
| `types/literal/numericStringLiteralTypes.ts` | not supported | unexpected character `&`, on `` type T0 = string & `${string}`;  // string `` |
| `types/literal/stringEnumLiteralTypes1.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2393, TS2345 |
| `types/literal/stringEnumLiteralTypes2.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2393, TS2345 |
| `types/literal/stringEnumLiteralTypes3.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum Choice { Unknown = "", Yes = "yes", No = "no" }; ` |
| `types/literal/stringLiteralsAssertionsInEqualityComparisons01.ts` | not supported | `any` is not supported, on ` let c = "foo" == (<any>"bar"); ` |
| `types/literal/stringLiteralsAssertionsInEqualityComparisons02.ts` | not supported | unexpected character `&`, on ` type EnhancedString = string & { enhancements: any }; ` |
| `types/literal/stringLiteralsAssignedToStringMappings.ts` | not supported | expected type, on `` let y: Uppercase<Lowercase<`${number}`>> = null as unknown as (Uppercase<Lowe... `` |
| `types/literal/stringLiteralsWithSwitchStatements03.ts` | not supported | expected `)`, on ` case (x, y, ("baz")): ` |
| `types/literal/stringLiteralsWithSwitchStatements04.ts` | not supported | expected `:` after `case` label, on ` case "foo", x: ` |
| `types/literal/stringMappingDeferralInConditionalTypes.ts` | not supported | string mapping types |
| `types/literal/stringMappingOverPatternLiterals.ts` | not supported | string mapping types |
| `types/literal/stringMappingReduction.ts` | not supported | string mapping types |
| `types/literal/templateLiteralTypes1.ts` | not supported | template literal types |
| `types/literal/templateLiteralTypes2.ts` | not supported | template literal types |
| `types/literal/templateLiteralTypes3.ts` | not supported | template literal types |
| `types/literal/templateLiteralTypes4.ts` | not supported | template literal types |
| `types/literal/templateLiteralTypes5.ts` | not supported | template literal types |
| `types/literal/templateLiteralTypes6.ts` | not supported | template literal types |
| `types/literal/templateLiteralTypes7.ts` | not supported | template literal types |
| `types/literal/templateLiteralTypes8.ts` | not supported | template literal types |
| `types/literal/templateLiteralTypesPatterns.ts` | not supported | template literal types |
| `types/literal/templateLiteralTypesPatternsPrefixSuffixAssignability.ts` | not supported | template literal types |
| `types/localTypes/localTypes1.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `types/localTypes/localTypes2.ts` | the port changes what it checks | `tsc` then reports TS2577, TS7022 |
| `types/localTypes/localTypes3.ts` | the port changes what it checks | `tsc` then reports TS2577, TS7022 |
| `types/localTypes/localTypes4.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/localTypes/localTypes5.ts` | the port changes what it checks | `tsc` then reports TS1144, TS1005, TS1003, TS1109, TS1390, TS1136, TS1128, TS2503, TS2304, TS7010, TS2365, TS2693, TS2552, TS2349, TS2451 |
| `types/mapped/` | not supported | mapped types |
| `types/members/augmentedTypeAssignmentCompatIndexSignature.ts` | not supported | index signatures |
| `types/members/augmentedTypeBracketAccessIndexSignature.ts` | not supported | index signatures |
| `types/members/classWithPrivateProperty.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `types/members/classWithProtectedProperty.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `types/members/classWithPublicProperty.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `types/members/duplicateNumericIndexers.ts` | not supported | index signatures |
| `types/members/duplicatePropertyNames.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/members/duplicateStringIndexers.ts` | not supported | index signatures |
| `types/members/indexSignatures1.ts` | not supported | unexpected character `&`, on `` let combo: { [x: `foo-${string}`]: 'a' \| 'b' } & { [x: `${string}-bar`]: 'b' ... `` |
| `types/members/objectTypeHidingMembersOfExtendedObject.ts` | the port changes what it checks | `tsc` then reports TS7053 |
| `types/members/objectTypeHidingMembersOfObjectAssignmentCompat.ts` | not supported | `as` to `I` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let i: I = null as unknown as (I); ` |
| `types/members/objectTypeHidingMembersOfObjectAssignmentCompat2.ts` | not supported | `as` to `I` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let i: I = null as unknown as (I); ` |
| `types/members/objectTypePropertyAccess.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/members/objectTypeWithCallSignatureAppearsToBeFunctionType.ts` | not supported | `any` is not supported, on ` let r2b: (x: any, y?: any) => any = i.apply; ` |
| `types/members/objectTypeWithCallSignatureHidingMembersOfExtendedFunction.ts` | the port changes what it checks | `tsc` then reports TS7053 |
| `types/members/objectTypeWithCallSignatureHidingMembersOfFunction.ts` | not supported | `any` is not supported, on ` apply(a: any, b?: any): void; ` |
| `types/members/objectTypeWithCallSignatureHidingMembersOfFunctionAssignmentCompat.ts` | not supported | expected field name in object type, on ` (): void ` |
| `types/members/objectTypeWithConstructSignatureAppearsToBeFunctionType.ts` | not supported | construct signatures |
| `types/members/objectTypeWithConstructSignatureHidingMembersOfExtendedFunction.ts` | not supported | construct signatures |
| `types/members/objectTypeWithConstructSignatureHidingMembersOfFunction.ts` | not supported | construct signatures |
| `types/members/objectTypeWithConstructSignatureHidingMembersOfFunctionAssignmentCompat.ts` | not supported | construct signatures |
| `types/members/objectTypeWithDuplicateNumericProperty.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `types/members/objectTypeWithNumericProperty.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/members/objectTypeWithStringAndNumberIndexSignatureToAny.ts` | not supported | index signatures |
| `types/members/objectTypeWithStringIndexerHidingObjectIndexer.ts` | not supported | index signatures |
| `types/members/objectTypeWithStringNamedNumericProperty.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2448, TS2454 |
| `types/members/objectTypeWithStringNamedPropertyOfIllegalCharacters.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2551 |
| `types/members/typesWithPrivateConstructor.ts` | not supported | expected `{`, on ` private constructor(x: number); ` |
| `types/members/typesWithProtectedConstructor.ts` | not supported | `protected` is not supported, on ` protected constructor() { } ` |
| `types/members/typesWithPublicConstructor.ts` | not supported | expected `{`, on ` public constructor(x: number); ` |
| `types/members/typesWithSpecializedCallSignatures.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/members/typesWithSpecializedConstructSignatures.ts` | not supported | construct signatures |
| `types/namedTypes/classWithOnlyPublicMembersEquivalentToInterface.ts` | not supported | parameter requires a type annotation, on ` public set z(v) { } ` |
| `types/namedTypes/classWithOnlyPublicMembersEquivalentToInterface2.ts` | not supported | parameter requires a type annotation, on ` public set z(v) { } ` |
| `types/namedTypes/classWithOptionalParameter.ts` | not supported | expected `:` and a type for the class field, on ` f?(): void {} ` |
| `types/namedTypes/interfaceWithPrivateMember.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` private x: string; ` |
| `types/namedTypes/optionalMethods.ts` | not supported | optional interface methods are not supported, on ` g?(): number; ` |
| `types/never/neverInference.ts` | the port changes what it checks | `tsc` then reports TS2315, TS2300 |
| `types/never/neverIntersectionNotCallable.ts` | not supported | intersection types |
| `types/never/neverTypeErrors2.ts` | duplicate | of `types/never/neverTypeErrors1.ts` |
| `types/never/neverUnionIntersection.ts` | not supported | intersection types |
| `types/nonPrimitive/` | not supported | the `object` type |
| `types/objectTypeLiteral/callSignatures/callSignaturesThatDifferOnlyByReturnType.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/objectTypeLiteral/callSignatures/callSignaturesThatDifferOnlyByReturnType2.ts` | not supported | `as` to `A` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let x: A = null as unknown as (A); ` |
| `types/objectTypeLiteral/callSignatures/callSignaturesThatDifferOnlyByReturnType3.ts` | not supported | duplicate declaration of interface `I`, on ` interface I { ` |
| `types/objectTypeLiteral/callSignatures/callSignaturesWithAccessibilityModifiersOnParameters.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7020, TS7010 |
| `types/objectTypeLiteral/callSignatures/callSignaturesWithDuplicateParameters.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7020, TS7010 |
| `types/objectTypeLiteral/callSignatures/callSignaturesWithOptionalParameters.ts` | the port changes what it checks | `tsc` then reports TS7020, TS7010 |
| `types/objectTypeLiteral/callSignatures/callSignaturesWithOptionalParameters2.ts` | the port changes what it checks | `tsc` then reports TS7010, TS7020 |
| `types/objectTypeLiteral/callSignatures/callSignaturesWithParameterInitializers.ts` | the port changes what it checks | `tsc` then reports TS7020, TS7010 |
| `types/objectTypeLiteral/callSignatures/callSignaturesWithParameterInitializers2.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/objectTypeLiteral/callSignatures/callSignatureWithOptionalParameterAndInitializer.ts` | the port changes what it checks | `tsc` then reports TS7020, TS7010 |
| `types/objectTypeLiteral/callSignatures/callSignatureWithoutAnnotationsOrBody.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7020, TS7010 |
| `types/objectTypeLiteral/callSignatures/callSignatureWithoutReturnTypeAnnotationInference.ts` | the port changes what it checks | `tsc` then reports TS7006, TS2322 |
| `types/objectTypeLiteral/callSignatures/constructSignatureWithAccessibilityModifiersOnParameters.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/objectTypeLiteral/callSignatures/constructSignatureWithAccessibilityModifiersOnParameters2.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/objectTypeLiteral/callSignatures/identicalCallSignatures.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/objectTypeLiteral/callSignatures/identicalCallSignatures2.ts` | checks too little | 0 after the port |
| `types/objectTypeLiteral/callSignatures/identicalCallSignatures3.ts` | not supported | duplicate declaration of interface `I`, on ` interface I { ` |
| `types/objectTypeLiteral/callSignatures/parametersWithNoAnnotationAreAny.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7010 |
| `types/objectTypeLiteral/callSignatures/restParametersOfNonArrayTypes.ts` | the port changes what it checks | `tsc` then reports TS7020, TS7010 |
| `types/objectTypeLiteral/callSignatures/restParametersOfNonArrayTypes2.ts` | the port changes what it checks | `tsc` then reports TS7020, TS7010 |
| `types/objectTypeLiteral/callSignatures/restParametersWithArrayTypeAnnotations.ts` | the port changes what it checks | `tsc` then reports TS7020, TS7010 |
| `types/objectTypeLiteral/callSignatures/restParameterWithoutAnnotationIsAnyArray.ts` | the port changes what it checks | `tsc` then reports TS7019, TS7020, TS7010 |
| `types/objectTypeLiteral/callSignatures/specializedSignatureIsNotSubtypeOfNonSpecializedSignature.ts` | the port changes what it checks | `tsc` then reports TS7010, TS7020 |
| `types/objectTypeLiteral/callSignatures/specializedSignatureIsSubtypeOfNonSpecializedSignature.ts` | the port changes what it checks | `tsc` then reports TS7010, TS7020 |
| `types/objectTypeLiteral/callSignatures/specializedSignatureWithOptional.ts` | the port changes what it checks | `tsc` then reports TS2393 |
| `types/objectTypeLiteral/callSignatures/stringLiteralTypesInImplementationSignatures.ts` | the port changes what it checks | `tsc` then reports TS7020, TS7010 |
| `types/objectTypeLiteral/callSignatures/stringLiteralTypesInImplementationSignatures2.ts` | the port changes what it checks | `tsc` then reports TS7010, TS7020 |
| `types/objectTypeLiteral/callSignatures/typeParameterUsedAsTypeParameterConstraint.ts` | not supported | expected `,` or `>`, on ` function foo<T, U extends T>(x: T, y: U): T { ` |
| `types/objectTypeLiteral/callSignatures/typeParameterUsedAsTypeParameterConstraint2.ts` | not supported | expected `,` or `>`, on ` function foo<T, U extends T>(x: T, y: U): void { ` |
| `types/objectTypeLiteral/callSignatures/typeParameterUsedAsTypeParameterConstraint3.ts` | not supported | expected `,` or `>`, on ` foo<W extends V>(x: W): T; ` |
| `types/objectTypeLiteral/callSignatures/typeParameterUsedAsTypeParameterConstraint4.ts` | not supported | expected `,` or `>`, on ` class C<T, U extends T, V extends U> { ` |
| `types/objectTypeLiteral/constructSignatures/` | not supported | construct signatures |
| `types/objectTypeLiteral/indexSignatures/` | not supported | index signatures |
| `types/objectTypeLiteral/methodSignatures/functionLiterals.ts` | not supported | expected field name in object type, on ` func3: { (x: number): number };   // Object type literal ` |
| `types/objectTypeLiteral/methodSignatures/methodSignaturesWithOverloads.ts` | not supported | expected field name in object type, on ` (x: number): number; ` |
| `types/objectTypeLiteral/methodSignatures/methodSignaturesWithOverloads2.ts` | not supported | expected field name in object type, on ` (x: number): number; ` |
| `types/objectTypeLiteral/methodSignatures/objectTypesWithOptionalProperties.ts` | not supported | expected `,` or `}`, on ` x?: 1 // error ` |
| `types/objectTypeLiteral/methodSignatures/objectTypesWithOptionalProperties2.ts` | the port changes what it checks | `tsc` then reports TS7010, TS7008 |
| `types/objectTypeLiteral/objectTypeLiteralSyntax2.ts` | not supported | expected `;`, `,`, or `}`, on ` bar: string ` |
| `types/objectTypeLiteral/propertySignatures/numericNamedPropertyDuplicates.ts` | not supported | expected class member name, on ` 1: number; ` |
| `types/objectTypeLiteral/propertySignatures/numericStringNamedPropertyEquivalence.ts` | not supported | expected class member name, on ` 1.0: number; ` |
| `types/objectTypeLiteral/propertySignatures/propertyNamesOfReservedWords.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `types/objectTypeLiteral/propertySignatures/propertyNameWithoutTypeAnnotation.ts` | the port changes what it checks | `tsc` then reports TS7008, TS2352 |
| `types/primitives/boolean/assignFromBooleanInterface.ts` | not supported | `as` to `Boolean` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let a: Boolean = null as unknown as (Boolean); ` |
| `types/primitives/boolean/assignFromBooleanInterface2.ts` | not supported | duplicate declaration of interface `Boolean`, on ` interface Boolean { ` |
| `types/primitives/boolean/extendBooleanInterface.ts` | not supported | adding to a built-in interface |
| `types/primitives/boolean/invalidBooleanAssignments.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `types/primitives/boolean/validBooleanAssignments.ts` | not supported | `any` is not supported, on ` let a: any = x; ` |
| `types/primitives/enum/invalidEnumAssignments.ts` | not supported | `as` to `E` is not yet supported: enum targets need a per-variant value check at runtime, on ` let e: E = null as unknown as (E); ` |
| `types/primitives/enum/validEnumAssignments.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `types/primitives/null/directReferenceToNull.ts` | checks too little | 2 after the port |
| `types/primitives/null/validNullAssignments.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `types/primitives/number/assignFromNumberInterface.ts` | not supported | `as` to `Number` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let a: Number = null as unknown as (Number); ` |
| `types/primitives/number/assignFromNumberInterface2.ts` | not supported | optional function parameters are not yet supported, on ` toString(radix?: number): string; ` |
| `types/primitives/number/extendNumberInterface.ts` | not supported | adding to a built-in interface |
| `types/primitives/number/invalidNumberAssignments.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `types/primitives/string/assignFromStringInterface.ts` | not supported | `as` to `String` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let a: String = null as unknown as (String); ` |
| `types/primitives/string/assignFromStringInterface2.ts` | not supported | optional function parameters are not yet supported, on ` indexOf(searchString: string, position?: number): number; ` |
| `types/primitives/string/extendStringInterface.ts` | not supported | adding to a built-in interface |
| `types/primitives/string/invalidStringAssignments.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `types/primitives/string/validStringAssignments.ts` | not supported | `any` is not supported, on ` let a: any = x; ` |
| `types/primitives/stringLiteral/stringLiteralType.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/primitives/undefined/` | not supported | `undefined` |
| `types/primitives/void/invalidAssignmentsToVoid.ts` | not supported | expected `:` and a type for the class field, on ` class C { foo!: string; } ` |
| `types/primitives/void/invalidVoidAssignments.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `types/primitives/void/invalidVoidValues.ts` | not supported | expected `:` and a type for the class field, on ` class C { foo!: string } ` |
| `types/primitives/void/validVoidAssignments.ts` | not supported | `any` is not supported, on ` let y: any = null as unknown as (any); ` |
| `types/primitives/void/validVoidValues.ts` | not supported | cannot cast `unknown` to `void`: no assignable direction between these types, on ` let x: void = null as unknown as (void); ` |
| `types/rest/genericObjectRest.ts` | not supported | expected `,` or `>`, on ` function f1<T extends { a: string, b: number }>(obj: T): void { ` |
| `types/rest/genericRestArity.ts` | not supported | expected `,` or `>`, on ` function call<TS extends unknown[]>( ` |
| `types/rest/genericRestArityStrict.ts` | not supported | expected `,` or `>`, on ` function call<TS extends unknown[]>( ` |
| `types/rest/genericRestParameters1.ts` | not supported | rest parameter type must be an array, on ` let f1: (...x: [number, string, boolean]) => void = null as unknown as ((...x... ` |
| `types/rest/genericRestParameters2.ts` | not supported | rest elements in tuple types are not supported, on ` const t1: [number, string, ...boolean[]] = null as unknown as ([number, strin... ` |
| `types/rest/genericRestParameters3.ts` | not supported | rest parameter type must be an array, on ` let f1: (x: string, ...args: [string] \| [number, boolean]) => void = null as ... ` |
| `types/rest/objectRest.ts` | the port changes what it checks | `tsc` then reports TS2451, TS7008 |
| `types/rest/objectRest2.ts` | not supported | `any` is not supported, on ` function connectionFromArray(objects: number, args: any): {} { return null as... ` |
| `types/rest/objectRestAssignment.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `types/rest/objectRestCatchES5.ts` | the port changes what it checks | `tsc` then reports TS2339, TS2700 |
| `types/rest/objectRestForOf.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (let { x, ...restOf } of array) { ` |
| `types/rest/objectRestNegative.ts` | not supported | expected expression, on ` let { ...mustBeLast, a } = o; ` |
| `types/rest/objectRestParameter.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `types/rest/objectRestParameterES5.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `types/rest/objectRestPropertyMustBeLast.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/rest/objectRestReadonly.ts` | the port changes what it checks | `tsc` then reports TS2790 |
| `types/rest/restTuplesFromContextualTypes.ts` | not supported | expected expression, on ` (function (a, b, c){})(...t1); ` |
| `types/specifyingTypes/predefinedTypes/objectTypesWithPredefinedTypesAsName2.ts` | porter failure | still pruning after 40 passes |
| `types/specifyingTypes/typeLiterals/arrayLiteral.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/specifyingTypes/typeLiterals/arrayOfFunctionTypes3.ts` | not supported | expected `(` after constructor name in `new` expression, on ` let r3 = new y[0](); ` |
| `types/specifyingTypes/typeLiterals/arrayTypeOfFunctionTypes.ts` | the port changes what it checks | `tsc` then reports TS7053, TS7009 |
| `types/specifyingTypes/typeLiterals/arrayTypeOfFunctionTypes2.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/specifyingTypes/typeLiterals/arrayTypeOfTypeOf.ts` | not supported | `let` declaration requires an initializer, on ` let xs3: typeof Array<number> = null as unknown as (typeof Array<number>); ` |
| `types/specifyingTypes/typeLiterals/functionLiteral.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/specifyingTypes/typeLiterals/functionLiteralForOverloads.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/specifyingTypes/typeLiterals/functionLiteralForOverloads2.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/specifyingTypes/typeLiterals/parenthesizedTypes.ts` | the port changes what it checks | `tsc` then reports TS2451, TS7051 |
| `types/specifyingTypes/typeLiterals/unionTypeLiterals.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/specifyingTypes/typeQueries/invalidTypeOfTarget.ts` | not supported | expected a value name after `typeof`, on ` let x1: typeof = null as unknown as (typeof) {}; ` |
| `types/specifyingTypes/typeQueries/recursiveTypesWithTypeof.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/specifyingTypes/typeQueries/typeofAnExportedType.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/specifyingTypes/typeQueries/typeofANonExportedType.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/specifyingTypes/typeQueries/typeofClass2.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7010 |
| `types/specifyingTypes/typeQueries/typeofClassWithPrivates.ts` | not supported | `as` to `C<string>` is not yet supported: class types aren't yet supported as `as` targets, on ` let c: C<string> = null as unknown as (C<string>); ` |
| `types/specifyingTypes/typeQueries/typeofModuleWithoutExports.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `types/specifyingTypes/typeQueries/typeofThis.ts` | the port changes what it checks | `tsc` then reports TS18047 |
| `types/specifyingTypes/typeQueries/typeofThisWithImplicitThis.ts` | the port changes what it checks | `tsc` then reports TS2683 |
| `types/specifyingTypes/typeQueries/typeofTypeParameter.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `types/specifyingTypes/typeQueries/typeQueryOnClass.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7010 |
| `types/specifyingTypes/typeQueries/typeQueryWithReservedWords.ts` | not supported | `let` is a reserved keyword and can't be used as a name, on ` let: typeof Controller.prototype.let;        // Should not error ` |
| `types/specifyingTypes/typeReferences/genericTypeReferenceWithoutTypeArgument.d.ts` | the port changes what it checks | `tsc` then reports TS1046, TS1039, TS1183 |
| `types/specifyingTypes/typeReferences/genericTypeReferenceWithoutTypeArgument3.ts` | not supported | expected `;` after expression, on ` declare class C<T> { ` |
| `types/spread/objectSpreadComputedProperty.ts` | not supported | `any` is not supported, on ` let a: any = null; ` |
| `types/spread/objectSpreadNegativeParse.ts` | the port changes what it checks | `tsc` then reports TS2554 |
| `types/spread/objectSpreadNoTransform.ts` | not supported | `let` declaration requires an initializer, on ` let b; ` |
| `types/spread/objectSpreadSetonlyAccessor.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/spread/spreadMethods.ts` | not supported | class fields require a type annotation, on ` p = 12; ` |
| `types/spread/spreadNonObject1.ts` | not supported | expected type, on `` type S = `${number}`; `` |
| `types/spread/spreadNonPrimitive.ts` | not supported | unknown type `object`, on ` let o: object = null as unknown as (object); ` |
| `types/spread/spreadObjectOrFalsy.ts` | not supported | unexpected character `&`, on ` function f1<T>(a: T & null): any { ` |
| `types/spread/spreadTypeVariable.ts` | not supported | expected `,` or `>`, on ` function f1<T extends number>(arg: T): any { ` |
| `types/stringLiteral/stringLiteralTypesAsTypeParameterConstraint01.ts` | not supported | expected `,` or `>`, on ` function foo<T extends "foo">(f: (x: T) => T): (x: T) => T { ` |
| `types/stringLiteral/stringLiteralTypesAsTypeParameterConstraint02.ts` | not supported | expected `,` or `>`, on ` function foo<T extends "foo">(f: (x: T) => T): (x: T) => T { ` |
| `types/stringLiteral/stringLiteralTypesInUnionTypes01.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/stringLiteral/stringLiteralTypesInUnionTypes03.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/stringLiteral/stringLiteralTypesOverloadAssignability03.ts` | duplicate | of `types/stringLiteral/stringLiteralTypesOverloadAssignability01.ts` |
| `types/stringLiteral/stringLiteralTypesOverloadAssignability05.ts` | duplicate | of `types/stringLiteral/stringLiteralTypesOverloadAssignability01.ts` |
| `types/stringLiteral/stringLiteralTypesOverloads05.ts` | not supported | expected `{`, on ` function doThing(x: "dog"): Dog; ` |
| `types/stringLiteral/stringLiteralTypesTypePredicates01.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/stringLiteral/stringLiteralTypesWithTemplateStrings02.ts` | checks too little | 4 after the port |
| `types/stringLiteral/typeArgumentsWithStringLiteralTypes01.ts` | not supported | expected `;` after expression, on ` namespace n1 { ` |
| `types/thisType/` | not supported | `this` types |
| `types/tuple/arityAndOrderCompatibility01.ts` | not supported | expected interface member name, on ` 0: string; ` |
| `types/tuple/castingTuple.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/tuple/contextualTypeTupleEnd.ts` | not supported | rest elements in tuple types are not supported, on ` type Funcs = [...((arg: number) => void)[], (arg: string) => void]; ` |
| `types/tuple/emptyTuples/emptyTuplesTypeAssertion01.ts` | not supported | tuple types must have at least one element, on ` let x = <[]>[]; ` |
| `types/tuple/emptyTuples/emptyTuplesTypeAssertion02.ts` | not supported | tuple types must have at least one element, on ` let x = [] as []; ` |
| `types/tuple/named/namedTupleMembers.ts` | the port changes what it checks | `tsc` then reports TS7051 |
| `types/tuple/named/namedTupleMembersErrors.ts` | not supported | `any` is not supported, on ` export type List = [item: any, ...any]; ` |
| `types/tuple/named/partiallyNamedTuples.ts` | not supported | expected `]` to close array type, on ` function fb3(a: NamedAnonymousMixed, ...args: NamedAnonymousMixed[3]): void {} ` |
| `types/tuple/named/partiallyNamedTuples2.ts` | not supported | expected `,` or `>`, on ` interface MultiKeyMap<Keys extends readonly unknown[], Value> { ` |
| `types/tuple/named/partiallyNamedTuples3.ts` | not supported | expected expression, on ` const output = ((...args) => args)(...tuple); ` |
| `types/tuple/optionalTupleElements1.ts` | not supported | optional tuple elements are not supported, on ` type T2 = [number, string, boolean?]; ` |
| `types/tuple/restTupleElements1.ts` | not supported | optional tuple elements are not supported, on ` type T00 = [string?]; ` |
| `types/tuple/strictTupleLength.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/tuple/tupleElementTypes1.ts` | not supported | `any` is not supported, on ` let [a, b]: [number, any] = [null, null]; ` |
| `types/tuple/tupleElementTypes2.ts` | not supported | `any` is not supported, on ` function f([a, b]: [number, any]): void { } ` |
| `types/tuple/tupleElementTypes4.ts` | not supported | parameter requires a type annotation, on ` function f([a, b] = [0, null]): void { } ` |
| `types/tuple/tupleLengthCheck.ts` | not supported | rest elements in tuple types are not supported, on ` const rest: [number, string, ...boolean[]] = null as unknown as ([number, str... ` |
| `types/tuple/typeInferenceWithTupleType.ts` | not supported | expected expression, on ` for (let i = 0; i < length; ++i) { ` |
| `types/tuple/unionsOfTupleTypes1.ts` | not supported | rest elements in tuple types are not supported, on ` type T3 = [string, ...number[]]; ` |
| `types/tuple/variadicTuples1.ts` | the port changes what it checks | `tsc` then reports TS2393 |
| `types/tuple/variadicTuples2.ts` | not supported | rest elements in tuple types are not supported, on ` type V00 = [number, ...string[]]; ` |
| `types/tuple/variadicTuples3.ts` | not supported | expected `,` or `>`, on ` function test1<T extends any[], P extends any[]>(): [...T, ...P] { ` |
| `types/tuple/wideningTuples1.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/tuple/wideningTuples2.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/tuple/wideningTuples3.ts` | not supported | `any` is not supported, on ` let a: [any] = null as unknown as ([any]); ` |
| `types/tuple/wideningTuples4.ts` | not supported | `any` is not supported, on ` let a: [any] = null as unknown as ([any]); ` |
| `types/tuple/wideningTuples6.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/typeAliases/asiPreventsParsingAsTypeAlias01.ts` | not supported | `let` declaration requires an initializer, on ` let type; ` |
| `types/typeAliases/asiPreventsParsingAsTypeAlias02.ts` | the port changes what it checks | `tsc` then reports TS7034, TS7005 |
| `types/typeAliases/circularTypeAliasForUnionWithClass.ts` | not supported | expected class member name, on ` [x: number]: T3; ` |
| `types/typeAliases/circularTypeAliasForUnionWithInterface.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeAliases/classDoesNotDependOnBaseTypes.ts` | not supported | expected class member name, on ` [n: number]: StringTree; ` |
| `types/typeAliases/directDependenceBetweenTypeAliases.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeAliases/intrinsicKeyword.ts` | not supported | string mapping types |
| `types/typeAliases/intrinsicTypes.ts` | not supported | string mapping types |
| `types/typeAliases/reservedNamesInAliases.ts` | porter failure | nothing to prune at offsets 110, 110 |
| `types/typeAliases/typeAliases.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeAliases/typeAliasesForObjectTypes.ts` | checks too little | 2 after the port |
| `types/typeParameters/recurringTypeParamForContainerOfBase01.ts` | not supported | expected `,` or `>`, on ` interface BoxOfFoo<T extends Foo<T>> { ` |
| `types/typeParameters/typeArgumentLists/callNonGenericFunctionWithTypeArguments.ts` | the port changes what it checks | `tsc` then reports TS7010, TS2451, TS2722, TS18048 |
| `types/typeParameters/typeArgumentLists/constraintSatisfactionWithAny.ts` | not supported | expected `,` or `>`, on ` function foo<T extends String>(x: T): T { return null; } ` |
| `types/typeParameters/typeArgumentLists/constraintSatisfactionWithAny2.ts` | not supported | expected `,` or `>`, on ` function foo<Z, T extends <U>(x: U) => Z>(y: T): Z { return null as unknown a... ` |
| `types/typeParameters/typeArgumentLists/constraintSatisfactionWithEmptyObject.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeParameters/typeArgumentLists/functionConstraintSatisfaction.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeParameters/typeArgumentLists/functionConstraintSatisfaction2.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeParameters/typeArgumentLists/functionConstraintSatisfaction3.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeParameters/typeArgumentLists/instantiateGenericClassWithZeroTypeArguments.ts` | checks too little | 4 after the port |
| `types/typeParameters/typeArgumentLists/instantiateNonGenericTypeWithTypeArguments.ts` | the port changes what it checks | `tsc` then reports TS7009, TS2451 |
| `types/typeParameters/typeArgumentLists/instantiationExpressionErrors.ts` | not supported | expected field name in object type, on ` let f: { <T>(): T, g<U>(): U } = null as unknown as ({ <T>(): T, g<U>(): U }); ` |
| `types/typeParameters/typeArgumentLists/instantiationExpressions.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeParameters/typeArgumentLists/typeParameterAsTypeParameterConstraint.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeParameters/typeArgumentLists/typeParameterAsTypeParameterConstraint2.ts` | not supported | expected `,` or `>`, on ` function foo<T, U extends T>(x: T, y: U): U { return y; } // this is now an e... ` |
| `types/typeParameters/typeArgumentLists/typeParameterAsTypeParameterConstraintTransitively.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/typeParameters/typeArgumentLists/typeParameterAsTypeParameterConstraintTransitively2.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/typeParameters/typeArgumentLists/wrappedAndRecursiveConstraints.ts` | not supported | expected `,` or `>`, on ` class C<T extends Date> { ` |
| `types/typeParameters/typeArgumentLists/wrappedAndRecursiveConstraints2.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeParameters/typeArgumentLists/wrappedAndRecursiveConstraints3.ts` | not supported | expected `,` or `>`, on ` class C<T extends { length: number }> { ` |
| `types/typeParameters/typeArgumentLists/wrappedAndRecursiveConstraints4.ts` | not supported | expected `,` or `>`, on ` class C<T extends { length: number }> { ` |
| `types/typeParameters/typeParameterAsBaseType.ts` | checks too little | 4 after the port |
| `types/typeParameters/typeParameterLists/innerTypeParameterShadowingOuterOne.ts` | not supported | expected `,` or `>`, on ` function f<T extends Date>(): void { ` |
| `types/typeParameters/typeParameterLists/innerTypeParameterShadowingOuterOne2.ts` | not supported | expected `,` or `>`, on ` class C<T extends Date> { ` |
| `types/typeParameters/typeParameterLists/propertyAccessOnTypeParameterWithConstraints.ts` | not supported | expected `,` or `>`, on ` class C<T extends Date> { ` |
| `types/typeParameters/typeParameterLists/propertyAccessOnTypeParameterWithConstraints2.ts` | not supported | expected `,` or `>`, on ` class C<U extends A, T extends A> { ` |
| `types/typeParameters/typeParameterLists/propertyAccessOnTypeParameterWithConstraints3.ts` | not supported | expected `,` or `>`, on ` class C<U extends A, T extends U> { ` |
| `types/typeParameters/typeParameterLists/propertyAccessOnTypeParameterWithConstraints4.ts` | the port changes what it checks | `tsc` then reports TS7053, TS7022, TS2448 |
| `types/typeParameters/typeParameterLists/propertyAccessOnTypeParameterWithConstraints5.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448 |
| `types/typeParameters/typeParameterLists/propertyAccessOnTypeParameterWithoutConstraints.ts` | the port changes what it checks | `tsc` then reports TS7053, TS2339, TS2571 |
| `types/typeParameters/typeParameterLists/staticMembersUsingClassTypeParameter.ts` | not supported | expected `,` or `>`, on ` class C3<T extends Date> { ` |
| `types/typeParameters/typeParameterLists/typeParameterConstModifiers.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeParameters/typeParameterLists/typeParameterConstModifiersReturnsAndYields.ts` | the port changes what it checks | `tsc` then reports TS2322, TS2393, TS2554 |
| `types/typeParameters/typeParameterLists/typeParameterConstModifiersReverseMappedTypes.ts` | not supported | `const` is a reserved keyword and can't be used as a name, on ` function test1<const T>(obj: { ` |
| `types/typeParameters/typeParameterLists/typeParameterConstModifiersWithIntersection.ts` | not supported | intersection types |
| `types/typeParameters/typeParameterLists/typeParameterDirectlyConstrainedToItself.ts` | not supported | expected `,` or `>`, on ` class C<T extends T> { } ` |
| `types/typeParameters/typeParameterLists/typeParameterIndirectlyConstrainedToItself.ts` | not supported | expected `,` or `>`, on ` class C<U extends T, T extends U> { } ` |
| `types/typeParameters/typeParameterLists/typeParametersAvailableInNestedScope.ts` | not supported | class fields require a type annotation, on ` x = <U>(a: U) => { ` |
| `types/typeParameters/typeParameterLists/typeParametersAvailableInNestedScope2.ts` | checks too little | 2 after the port |
| `types/typeParameters/typeParameterLists/typeParametersAvailableInNestedScope3.ts` | not supported | expected type, on ` function foo<T>(v: T): { a: <T>(a: T) => T; b: () => T; c: <T>(v: T) => { a: ... ` |
| `types/typeParameters/typeParameterLists/typeParameterUsedAsConstraint.ts` | not supported | expected `,` or `>`, on ` class C<T, U extends T> { } ` |
| `types/typeParameters/typeParameterLists/varianceAnnotations.ts` | not supported | expected `,` or `>`, on ` type Covariant<out T> = { ` |
| `types/typeParameters/typeParameterLists/varianceAnnotationsWithCircularlyReferencesError.ts` | not supported | `in` is a reserved keyword and can't be used as a name, on ` type T1<in in> = T1 // Error: circularly references ` |
| `types/typeRelationships/apparentType/apparentTypeSubtyping.ts` | not supported | expected `,` or `>`, on ` class Base<U extends String> { ` |
| `types/typeRelationships/apparentType/apparentTypeSupertype.ts` | not supported | expected `,` or `>`, on ` class Derived<U extends String> extends Base { // error ` |
| `types/typeRelationships/assignmentCompatibility/anyAssignabilityInInheritance.ts` | the port changes what it checks | `tsc` then reports TS2393, TS2451, TS7006 |
| `types/typeRelationships/assignmentCompatibility/anyAssignableToEveryType.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `types/typeRelationships/assignmentCompatibility/anyAssignableToEveryType2.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithCallSignatures.ts` | not supported | expected field name in object type, on ` let a: { (x: number): void } = null as unknown as ({ (x: number): void }); ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithCallSignatures3.ts` | not supported | expected field name in object type, on ` (x: number): number[]; ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithCallSignatures4.ts` | not supported | expected `;` after expression, on ` namespace Errors { ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithCallSignatures5.ts` | not supported | expected type, on ` let a: <T>(x: T) => T[] = null as unknown as (<T>(x: T) => T[]); ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithCallSignatures6.ts` | not supported | expected type, on ` a: <T>(x: T) => T[]; ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithCallSignaturesWithOptionalParameters.ts` | not supported | optional function parameters are not yet supported, on ` a2: (x?: number) => number; ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithCallSignaturesWithRestParameters.ts` | not supported | optional function parameters are not yet supported, on ` a3: (x: number, y?: string, ...z: number[]) => number; ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithConstructSignatures.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithConstructSignatures2.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithConstructSignatures3.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithConstructSignatures4.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithConstructSignatures5.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithConstructSignatures6.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithConstructSignaturesWithOptionalParameters.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithDiscriminatedUnion.ts` | not supported | unexpected character `&`, on ` type TypeB = { kind: MyEnum.B } & ({ id?: null } \| { id: number }); ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithEnumIndexer.ts` | not supported | index signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithGenericCallSignatures.ts` | not supported | expected type, on ` let f: <S extends { p: string }[]>(x: S) => void = null as unknown as (<S ext... ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithGenericCallSignatures2.ts` | not supported | generic call signatures are not yet supported, on ` <T>(x: T, ...y: T[][]): void ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithGenericCallSignatures3.ts` | not supported | generic call signatures are not yet supported, on ` <U>(f: (x: T) => (y: S) => U): U ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithGenericCallSignatures4.ts` | not supported | expected type, on ` let x: <T extends I2<T>>(z: T) => void = null as unknown as (<T extends I2<T>... ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithGenericCallSignaturesWithOptionalParameters.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithNumericIndexer.ts` | not supported | index signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithNumericIndexer2.ts` | not supported | index signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithNumericIndexer3.ts` | not supported | index signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithObjectMembers.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448 |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithObjectMembers4.ts` | not supported | expected `;` after expression, on ` namespace OnlyDerived { ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithObjectMembers5.ts` | not supported | `as` to `C` is not yet supported: class types aren't yet supported as `as` targets, on ` let c: C = null as unknown as (C); ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithObjectMembersAccessibility.ts` | not supported | expected `;` after expression, on ` namespace TargetIsPublic { ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithObjectMembersNumericNames.ts` | not supported | expected class member name, on ` class S { 1: string; } ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithObjectMembersOptionality.ts` | not supported | expected `;` after expression, on ` namespace TargetHasOptional { ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithObjectMembersOptionality2.ts` | not supported | expected `;` after expression, on ` namespace TargetHasOptional { ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithObjectMembersStringNumericNames.ts` | not supported | expected `;` after expression, on ` namespace JustStrings { ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithStringIndexer.ts` | not supported | index signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithStringIndexer2.ts` | not supported | index signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithStringIndexer3.ts` | not supported | index signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithWithGenericConstructSignatures.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/callSignatureAssignabilityInInheritance.ts` | not supported | expected `;` after expression, on ` namespace CallSignature { ` |
| `types/typeRelationships/assignmentCompatibility/callSignatureAssignabilityInInheritance2.ts` | not supported | expected field name in object type, on ` (x: number): number[]; ` |
| `types/typeRelationships/assignmentCompatibility/callSignatureAssignabilityInInheritance3.ts` | not supported | expected `;` after expression, on ` namespace Errors { ` |
| `types/typeRelationships/assignmentCompatibility/callSignatureAssignabilityInInheritance4.ts` | not supported | expected type, on ` a: <T>(x: T) => T[]; ` |
| `types/typeRelationships/assignmentCompatibility/callSignatureAssignabilityInInheritance5.ts` | not supported | expected type, on ` a: <T>(x: T) => T[]; ` |
| `types/typeRelationships/assignmentCompatibility/callSignatureAssignabilityInInheritance6.ts` | not supported | expected type, on ` a: <T>(x: T) => T[]; ` |
| `types/typeRelationships/assignmentCompatibility/constructSignatureAssignabilityInInheritance.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeRelationships/assignmentCompatibility/constructSignatureAssignabilityInInheritance2.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeRelationships/assignmentCompatibility/constructSignatureAssignabilityInInheritance3.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeRelationships/assignmentCompatibility/constructSignatureAssignabilityInInheritance4.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeRelationships/assignmentCompatibility/constructSignatureAssignabilityInInheritance5.ts` | not supported | expected type, on ` a: new (x: number) => number[]; ` |
| `types/typeRelationships/assignmentCompatibility/constructSignatureAssignabilityInInheritance6.ts` | not supported | expected type, on ` a: new <T>(x: T) => T[]; ` |
| `types/typeRelationships/assignmentCompatibility/enumAssignability.ts` | not supported | expected `;` after expression, on ` namespace Others { ` |
| `types/typeRelationships/assignmentCompatibility/enumAssignabilityInInheritance.ts` | the port changes what it checks | `tsc` then reports TS2393, TS2345, TS2451, TS7006 |
| `types/typeRelationships/assignmentCompatibility/everyTypeAssignableToAny.ts` | not supported | `any` |
| `types/typeRelationships/assignmentCompatibility/genericCallWithObjectTypeArgsAndInitializers.ts` | not supported | expected `,` or `>`, on ` function foo3<T extends Number>(x: T = 1): void { } // error ` |
| `types/typeRelationships/assignmentCompatibility/intersectionIncludingPropFromGlobalAugmentation.ts` | not supported | unexpected character `&`, on ` type Test2 = Test1 & { optional?: unknown }; ` |
| `types/typeRelationships/assignmentCompatibility/nullAssignableToEveryType.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/typeRelationships/assignmentCompatibility/nullAssignedToUndefined.ts` | the port changes what it checks | `tsc` then reports TS2364, TS2304 |
| `types/typeRelationships/assignmentCompatibility/numberAssignableToEnum.ts` | not supported | `as` to `E` is not yet supported: enum targets need a per-variant value check at runtime, on ` let e: E = null as unknown as (E); ` |
| `types/typeRelationships/assignmentCompatibility/typeParameterAssignability2.ts` | not supported | expected `,` or `>`, on ` function foo<T, U extends T>(t: T, u: U): void { ` |
| `types/typeRelationships/assignmentCompatibility/typeParameterAssignability3.ts` | not supported | expected `,` or `>`, on ` function foo<T extends Foo, U extends Foo>(t: T, u: U): void { ` |
| `types/typeRelationships/bestCommonType/bestCommonTypeOfTuple2.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` interface base1 { i } ` |
| `types/typeRelationships/bestCommonType/functionWithMultipleReturnStatements.ts` | not supported | expected `,` or `>`, on ` function f8<T extends U, U extends V, V>(x: T, y: U): U { ` |
| `types/typeRelationships/bestCommonType/heterogeneousArrayLiterals.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/typeRelationships/comparable/equalityWithIntersectionTypes01.ts` | not supported | intersection types |
| `types/typeRelationships/comparable/equalityWithtNullishCoalescingAssignment.ts` | not supported | optional function parameters are not yet supported, on ` function f1(a?: boolean): void { ` |
| `types/typeRelationships/comparable/optionalProperties01.ts` | checks too little | 4 after the port |
| `types/typeRelationships/comparable/optionalProperties02.ts` | the port changes what it checks | `tsc` then reports TS2352 |
| `types/typeRelationships/comparable/switchCaseWithIntersectionTypes01.ts` | not supported | intersection types |
| `types/typeRelationships/comparable/typeAssertionsWithIntersectionTypes01.ts` | not supported | intersection types |
| `types/typeRelationships/instanceOf/narrowingConstrainedTypeVariable.ts` | not supported | expected `,` or `>`, on ` function f1<T extends C>(v: T \| string): void { ` |
| `types/typeRelationships/recursiveTypes/arrayLiteralsWithRecursiveGenerics.ts` | not supported | `as` to `List<number>` is not yet supported: class types aren't yet supported as `as` targets, on ` let list: List<number> = null as unknown as (List<number>); ` |
| `types/typeRelationships/recursiveTypes/infiniteExpansionThroughInstantiation.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `types/typeRelationships/recursiveTypes/infiniteExpansionThroughInstantiation2.ts` | not supported | expected `,` or `>`, on ` interface AA<T extends AA<T>> // now an error due to referencing type paramet... ` |
| `types/typeRelationships/recursiveTypes/infiniteExpansionThroughTypeInference.ts` | not supported | expected `;`, `,`, or `}` after interface member, on ` y: T ` |
| `types/typeRelationships/recursiveTypes/nominalSubtypeCheckOfTypeParameter.ts` | checks too little | 0 after the port |
| `types/typeRelationships/recursiveTypes/nominalSubtypeCheckOfTypeParameter2.ts` | checks too little | 0 after the port |
| `types/typeRelationships/recursiveTypes/recursiveTypeInGenericConstraint.ts` | not supported | expected `,` or `>`, on ` class Foo<T extends G<T>> { // error, constraint referencing itself ` |
| `types/typeRelationships/recursiveTypes/recursiveTypesUsedAsFunctionParameters.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/subtypesAndSuperTypes/enumIsNotASubtypeOfAnythingButNumber.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/typeRelationships/subtypesAndSuperTypes/nullIsSubtypeOfEverythingButUndefined.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2304 |
| `types/typeRelationships/subtypesAndSuperTypes/stringLiteralTypeIsSubtypeOfString.ts` | the port changes what it checks | `tsc` then reports TS7010, TS2322 |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfAny.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameter.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2304 |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameterWithConstraints.ts` | not supported | expected `,` or `>`, on ` class D1<T extends U, U> extends C3<T> { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameterWithConstraints2.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2304 |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameterWithConstraints3.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameterWithConstraints4.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameterWithRecursiveConstraints.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfUnion.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithCallSignatures.ts` | the port changes what it checks | `tsc` then reports TS2393 |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithCallSignatures2.ts` | the port changes what it checks | `tsc` then reports TS2393, TS2345 |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithCallSignatures3.ts` | the port changes what it checks | `tsc` then reports TS2393, TS2345 |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithCallSignatures4.ts` | the port changes what it checks | `tsc` then reports TS2393, TS2352 |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithCallSignaturesA.ts` | checks too little | 4 after the port |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithCallSignaturesWithOptionalParameters.ts` | not supported | optional function parameters are not yet supported, on ` a2: (x?: number) => number; ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithCallSignaturesWithRestParameters.ts` | not supported | optional function parameters are not yet supported, on ` a3: (x: number, y?: string, ...z: number[]) => number; ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithCallSignaturesWithSpecializedSignatures.ts` | not supported | expected `;` after expression, on ` namespace CallSignature { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithConstructSignatures.ts` | not supported | construct signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithConstructSignatures2.ts` | not supported | construct signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithConstructSignatures3.ts` | not supported | construct signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithConstructSignatures4.ts` | not supported | construct signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithConstructSignatures5.ts` | not supported | construct signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithConstructSignatures6.ts` | not supported | construct signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithConstructSignaturesWithOptionalParameters.ts` | not supported | construct signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithConstructSignaturesWithSpecializedSignatures.ts` | not supported | construct signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithGenericCallSignaturesWithOptionalParameters.ts` | not supported | expected `;` after expression, on ` namespace ClassTypeParam { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithGenericConstructSignaturesWithOptionalParameters.ts` | not supported | construct signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithNumericIndexer.ts` | not supported | index signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithNumericIndexer2.ts` | not supported | index signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithNumericIndexer3.ts` | not supported | index signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithNumericIndexer4.ts` | not supported | index signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithNumericIndexer5.ts` | not supported | index signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithObjectMembers.ts` | not supported | expected class member name, on ` 1: Base; ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithObjectMembers2.ts` | not supported | expected `;` after expression, on ` namespace NotOptional { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithObjectMembers3.ts` | not supported | expected `;` after expression, on ` namespace NotOptional { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithObjectMembers4.ts` | not supported | expected class member name, on ` 1: Base; ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithObjectMembers5.ts` | not supported | expected `;` after expression, on ` namespace NotOptional { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithObjectMembersAccessibility.ts` | not supported | expected `:` and a type for the class field, on ` public 1: Base; ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithObjectMembersAccessibility2.ts` | not supported | expected `;` after expression, on ` namespace ExplicitPublic { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithOptionalProperties.ts` | not supported | `new` expects a constructor; `ObjectConstructor` declares no `new` method, on ` let r = f({ s: new Object() }); // ok ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithStringIndexer.ts` | not supported | index signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithStringIndexer2.ts` | not supported | index signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithStringIndexer3.ts` | not supported | index signatures |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithStringIndexer4.ts` | not supported | index signatures |
| `types/typeRelationships/subtypesAndSuperTypes/undefinedIsSubtypeOfEverything.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `types/typeRelationships/subtypesAndSuperTypes/unionSubtypeIfEveryConstituentTypeIsSubtype.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentity.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentity2.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithCallSignatures.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithCallSignatures2.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithCallSignatures3.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithCallSignaturesDifferingParamCounts.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithCallSignaturesDifferingParamCounts2.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithCallSignaturesWithOverloads.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithComplexConstraints.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithConstructSignatures.ts` | not supported | construct signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithConstructSignatures2.ts` | not supported | construct signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithConstructSignaturesDifferingParamCounts.ts` | not supported | construct signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignatures.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignatures2.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesDifferingByConstraints.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesDifferingByConstraints2.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesDifferingByConstraints3.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesDifferingByReturnType.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesDifferingByReturnType2.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesDifferingTypeParameterCounts.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesDifferingTypeParameterCounts2.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesDifferingTypeParameterNames.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesOptionalParams.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesOptionalParams2.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesOptionalParams3.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericConstructSignaturesDifferingByConstraints.ts` | not supported | construct signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericConstructSignaturesDifferingByConstraints2.ts` | not supported | construct signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericConstructSignaturesDifferingByConstraints3.ts` | not supported | construct signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericConstructSignaturesDifferingByReturnType.ts` | not supported | construct signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericConstructSignaturesDifferingByReturnType2.ts` | not supported | construct signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericConstructSignaturesDifferingTypeParameterCounts.ts` | not supported | construct signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericConstructSignaturesDifferingTypeParameterNames.ts` | not supported | construct signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericConstructSignaturesOptionalParams.ts` | not supported | construct signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericConstructSignaturesOptionalParams2.ts` | not supported | construct signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericConstructSignaturesOptionalParams3.ts` | not supported | construct signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithNumericIndexers1.ts` | not supported | index signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithNumericIndexers2.ts` | not supported | index signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithNumericIndexers3.ts` | not supported | index signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithOptionality.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithPrivates.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithPrivates2.ts` | the port changes what it checks | `tsc` then reports TS7010, TS2451 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithPrivates3.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` interface T2 { z } ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithPublics.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithStringIndexers.ts` | not supported | index signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithStringIndexers2.ts` | not supported | index signatures |
| `types/typeRelationships/typeAndMemberIdentity/primtiveTypesAreIdentical.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/typeParametersAreIdenticalToThemselves.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/unionTypeIdentity.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeRelationships/typeInference/bivariantInferences.ts` | not supported | `this` is a reserved keyword and can't be used as a name, on ` equalsShallow<T>(this: ReadonlyArray<T>, other: ReadonlyArray<T>): boolean; ` |
| `types/typeRelationships/typeInference/contextualSignatureInstantiation.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeRelationships/typeInference/discriminatedUnionInference.ts` | not supported | expected field name in object type, on ` type Foo<A> = { type: "foo", (): A[] }; ` |
| `types/typeRelationships/typeInference/genericCallToOverloadedMethodWithOverloadedArguments.ts` | the port changes what it checks | `tsc` then reports TS2393 |
| `types/typeRelationships/typeInference/genericCallTypeArgumentInference.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeRelationships/typeInference/genericCallWithArrayLiteralArgs.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeRelationships/typeInference/genericCallWithConstraintsTypeArgumentInference.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeRelationships/typeInference/genericCallWithConstraintsTypeArgumentInference2.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `types/typeRelationships/typeInference/genericCallWithConstructorTypedArguments5.ts` | not supported | expected type, on ` function foo<T, U>(arg: { cb: new(t: T) => U }): U { ` |
| `types/typeRelationships/typeInference/genericCallWithFunctionTypedArguments.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeRelationships/typeInference/genericCallWithFunctionTypedArguments2.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeRelationships/typeInference/genericCallWithFunctionTypedArguments3.ts` | not supported | expected field name in object type, on ` (x: boolean): boolean; ` |
| `types/typeRelationships/typeInference/genericCallWithFunctionTypedArguments4.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeRelationships/typeInference/genericCallWithGenericSignatureArguments.ts` | the port changes what it checks | `tsc` then reports TS2345 |
| `types/typeRelationships/typeInference/genericCallWithGenericSignatureArguments2.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `types/typeRelationships/typeInference/genericCallWithGenericSignatureArguments3.ts` | the port changes what it checks | `tsc` then reports TS1263, TS2322 |
| `types/typeRelationships/typeInference/genericCallWithNonSymmetricSubtypes.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgs.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgs2.ts` | not supported | expected `,` or `>`, on ` function f<T extends Base, U extends Base>(a: { x: T; y: U }): (T \| U)[] { ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndConstraints.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndConstraints2.ts` | not supported | expected `,` or `>`, on ` function f<T extends Base>(x: { foo: T; bar: T }): T { ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndConstraints3.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndConstraints4.ts` | not supported | expected `,` or `>`, on ` function foo<T, U extends T>(t: T, t2: U): (x: T) => U { ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndConstraints5.ts` | not supported | expected `,` or `>`, on ` function foo<T, U extends T>(t: T, t2: U): (x: T) => U { ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndIndexers.ts` | not supported | index signatures |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndIndexersErrors.ts` | not supported | index signatures |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndNumericIndexer.ts` | not supported | index signatures |
| `types/typeRelationships/typeInference/genericCallWithOverloadedConstructorTypedArguments.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeRelationships/typeInference/genericCallWithOverloadedConstructorTypedArguments2.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeRelationships/typeInference/genericCallWithOverloadedFunctionTypedArguments.ts` | the port changes what it checks | `tsc` then reports TS2345, TS2451, TS2322 |
| `types/typeRelationships/typeInference/genericCallWithOverloadedFunctionTypedArguments2.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `types/typeRelationships/typeInference/genericCallWithTupleType.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeRelationships/typeInference/genericClassWithFunctionTypedMemberArguments.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeRelationships/typeInference/genericClassWithObjectTypeArgsAndConstraints.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeRelationships/typeInference/genericContextualTypes1.ts` | not supported | expected type, on ` const f00: <A>(x: A) => A[] = list; ` |
| `types/typeRelationships/typeInference/genericContextualTypes2.ts` | not supported | unexpected character `&`, on ` type LowInfer<T> = T & {}; ` |
| `types/typeRelationships/typeInference/genericContextualTypes3.ts` | not supported | unexpected character `&`, on ` type LowInfer<T> = T & {}; ` |
| `types/typeRelationships/typeInference/genericFunctionParameters.ts` | not supported | expected type, on ` function f1<T>(cb: <S>(x: S) => T): T { return null as unknown as (T); } ` |
| `types/typeRelationships/typeInference/indexSignatureTypeInference.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeRelationships/typeInference/keyofInferenceIntersectsResults.ts` | not supported | expected `,` or `>`, on ` function foo<T = X>(x: keyof T, y: keyof T): T { return null as unknown as (T... ` |
| `types/typeRelationships/typeInference/keyofInferenceLowerPriorityThanReturn.ts` | not supported | unexpected character `&`, on ` function insertOnConflictDoNothing<Req extends object, Def extends object>(_t... ` |
| `types/typeRelationships/typeInference/noInfer.ts` | not supported | unexpected character `&`, on `` type T05 = NoInfer<`foo${string}` & `${string}bar`>; `` |
| `types/typeRelationships/typeInference/noInferRedeclaration.ts` | multi-file or JavaScript |  |
| `types/typeRelationships/typeInference/unionAndIntersectionInference1.ts` | not supported | intersection types |
| `types/typeRelationships/typeInference/unionAndIntersectionInference2.ts` | not supported | intersection types |
| `types/typeRelationships/typeInference/unionAndIntersectionInference3.ts` | not supported | intersection types |
| `types/typeRelationships/typeInference/unionTypeInference.ts` | not supported | unexpected character `&`, on ` function f4<T>(x: string & T): T { return null as unknown as (T); } ` |
| `types/typeRelationships/widenedTypes/arrayLiteralWidened.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/typeRelationships/widenedTypes/initializersWidened.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/typeRelationships/widenedTypes/strictNullChecksNoWidening.ts` | not supported | expected expression, on ` let a3 = void 0; ` |
| `types/union/contextualTypeWithUnionTypeCallSignatures.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/union/contextualTypeWithUnionTypeIndexSignatures.ts` | not supported | index signatures |
| `types/union/contextualTypeWithUnionTypeMembers.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/union/contextualTypeWithUnionTypeObjectLiteral.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/union/discriminatedUnionTypes3.ts` | the port changes what it checks | it leaves an `undefined` it can't rewrite |
| `types/union/discriminatedUnionTypes4.ts` | the port changes what it checks | `tsc` then reports TS2345 |
| `types/union/unionTypeCallSignatures.ts` | the port changes what it checks | `tsc` then reports TS2448, TS2454 |
| `types/union/unionTypeCallSignatures2.ts` | not supported | optional function parameters are not yet supported, on ` (x: string, y?: string): boolean; ` |
| `types/union/unionTypeCallSignatures3.ts` | not supported | optional function parameters are not yet supported, on ` function f2(s?: string): void { } ` |
| `types/union/unionTypeCallSignatures4.ts` | not supported | optional function parameters are not yet supported, on ` type F1 = (a: string, b?: string) => void; ` |
| `types/union/unionTypeCallSignatures5.ts` | not supported | `this` is a reserved keyword and can't be used as a name, on ` (this: void, b?: number): void; ` |
| `types/union/unionTypeCallSignatures6.ts` | not supported | unexpected character `&`, on ` let x1: A & C & { ` |
| `types/union/unionTypeCallSignatures7.ts` | not supported | expected `,` or `>`, on ` interface Callable<Name extends string> { ` |
| `types/union/unionTypeConstructSignatures.ts` | not supported | construct signatures |
| `types/union/unionTypeEquivalence.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `types/union/unionTypeIndexSignature.ts` | not supported | index signatures |
| `types/union/unionTypePropertyAccessibility.ts` | not supported | `protected` is not supported, on ` protected member: string; ` |
| `types/union/unionTypeReduction.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/union/unionTypeReduction2.ts` | not supported | optional function parameters are not yet supported, on ` function f1(x: { f(): void }, y: { f(x?: string): void }): void { ` |
| `types/union/unionTypeWithIndexSignature.ts` | not supported | index signatures |
| `types/uniqueSymbol/` | not supported | `Symbol` |
| `types/unknown/unknownType2.ts` | the port changes what it checks | `tsc` then reports TS1335 |
| `types/witness/witness.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448, TS2300, TS2451, TS2394, TS7006 |
