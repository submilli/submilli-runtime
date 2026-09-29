# Upstream cases left out

Every upstream conformance case that isn't in the suite, and why. A directory (ending
in `/`) is left out whole, with its subdirectories, except for any case in the suite.
Written by `../typescript-baselines/port-suite.cjs`: change its lists or the porter,
not this file. Declaration files (`.d.ts`) aren't cases, and `.tsx` files aren't read.

3900 entries: 1703 not supported, 1019 multi-file or JavaScript, 717 the port changes what it checks, 99 duplicate, 293 checks too little, 69 porter failure.

| Case | Reason | Detail |
|:-----|:-------|:-------|
| `additionalChecks/noPropertyAccessFromIndexSignature1.ts` | not supported | index signatures |
| `ambient/ambientDeclarations.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7019, TS7008, TS7005 |
| `ambient/ambientDeclarationsExternal.ts` | multi-file or JavaScript |  |
| `ambient/ambientDeclarationsPatterns_merging1.ts` | multi-file or JavaScript |  |
| `ambient/ambientDeclarationsPatterns_merging2.ts` | multi-file or JavaScript |  |
| `ambient/ambientDeclarationsPatterns_merging3.ts` | multi-file or JavaScript |  |
| `ambient/ambientDeclarationsPatterns_tooManyAsterisks.ts` | not supported | expected `;` after expression, on ` declare module "too*many*asterisks" { } ` |
| `ambient/ambientDeclarationsPatterns.ts` | multi-file or JavaScript |  |
| `ambient/ambientEnumDeclaration1.ts` | not supported | expected `;` after expression, on ` declare enum E { ` |
| `ambient/ambientEnumDeclaration2.ts` | not supported | expected `;` after expression, on ` declare enum E { ` |
| `ambient/ambientErrors.ts` | the port changes what it checks | `tsc` then reports TS1128, TS2393, TS7005 |
| `ambient/ambientExternalModuleInsideNonAmbient.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `ambient/ambientExternalModuleInsideNonAmbientExternalModule.ts` | not supported | expected a declaration after `export`, on ` export declare module "M" { } ` |
| `ambient/ambientExternalModuleMerging.ts` | multi-file or JavaScript |  |
| `ambient/ambientInsideNonAmbient.ts` | the port changes what it checks | `tsc` then reports TS7005 |
| `ambient/ambientInsideNonAmbientExternalModule.ts` | the port changes what it checks | `tsc` then reports TS7005 |
| `ambient/ambientModuleDeclarationWithReservedIdentifierInDottedPath.ts` | porter failure | nothing to prune at offsets 316; our first unsupported error: expected `;` after expression |
| `ambient/ambientModuleDeclarationWithReservedIdentifierInDottedPath2.ts` | porter failure | nothing to prune at offsets 260; our first unsupported error: expected `;` after expression |
| `ambient/ambientShorthand_declarationEmit.ts` | not supported | expected `;` after expression, on ` declare module "foo"; ` |
| `ambient/ambientShorthand_duplicate.ts` | multi-file or JavaScript |  |
| `ambient/ambientShorthand_merging.ts` | multi-file or JavaScript |  |
| `ambient/ambientShorthand_reExport.ts` | multi-file or JavaScript |  |
| `ambient/ambientShorthand.ts` | multi-file or JavaScript |  |
| `async/` | not supported | async/await |
| `asyncGenerators/` | not supported | async/await and generators |
| `classes/awaitAndYieldInProperty.ts` | the port changes what it checks | `tsc` then reports TS1003, TS1005, TS1128, TS2304, TS2693, TS2364 |
| `classes/classDeclarations/classAbstractKeyword/classAbstractAccessor.ts` | the port changes what it checks | `tsc` then reports TS7033 |
| `classes/classDeclarations/classAbstractKeyword/classAbstractAsIdentifier.ts` | not supported | expected `(` after constructor name in `new` expression, on ` new abstract; ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractAssignabilityConstructorFunction.ts` | not supported | expected `;` after expression, on ` abstract class A { } ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractClinterfaceAssignability.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `classes/classDeclarations/classAbstractKeyword/classAbstractConstructor.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractConstructorAssignability.ts` | not supported | expected `;` after expression, on ` abstract class B extends A {} ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractCrashedOnce.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `classes/classDeclarations/classAbstractKeyword/classAbstractExtends.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `classes/classDeclarations/classAbstractKeyword/classAbstractFactoryFunction.ts` | not supported | expected `;` after expression, on ` abstract class B extends A {} ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractGeneric.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `classes/classDeclarations/classAbstractKeyword/classAbstractImportInstantiation.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractInAModule.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractInheritance1.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `classes/classDeclarations/classAbstractKeyword/classAbstractInheritance2.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractInstantiations1.ts` | not supported | expected `;` after expression, on ` abstract class A {} ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractInstantiations2.ts` | not supported | expected `;` after expression, on ` abstract class B { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractManyKeywords.ts` | porter failure | nothing to prune at offsets 81; our first unsupported error: `export default` is not supported |
| `classes/classDeclarations/classAbstractKeyword/classAbstractMergedDeclaration.ts` | not supported | expected `;` after expression, on ` abstract class CM {} ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractMethodInNonAbstractClass.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `classes/classDeclarations/classAbstractKeyword/classAbstractMethodWithImplementation.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractMixedWithModifiers.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `classes/classDeclarations/classAbstractKeyword/classAbstractOverloads.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `classes/classDeclarations/classAbstractKeyword/classAbstractOverrideWithAbstract.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `classes/classDeclarations/classAbstractKeyword/classAbstractProperties.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractSingleLineDecl.ts` | not supported | expected `;` after expression, on ` abstract class A {} ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractSuperCalls.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `classes/classDeclarations/classAbstractKeyword/classAbstractUsingAbstractMethod1.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractUsingAbstractMethods2.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `classes/classDeclarations/classAbstractKeyword/classAbstractWithInterface.ts` | not supported | expected `;` after expression, on ` abstract interface I {} ` |
| `classes/classDeclarations/classAndInterfaceMergeConflictingMembers.ts` | not supported | expected `;` after expression, on ` declare class C1 { ` |
| `classes/classDeclarations/classAndInterfaceWithSameName.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `classes/classDeclarations/classAndVariableWithSameName.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `classes/classDeclarations/classBody/classBodyWithStatements.ts` | not supported | expected `:` and a type for the class field, on ` let x = 1; ` |
| `classes/classDeclarations/classBody/classWithEmptyBody.ts` | not supported | `as` to `C` is not yet supported: class types aren't yet supported as `as` targets, on ` let c: C = null as unknown as (C); ` |
| `classes/classDeclarations/classDeclarationLoop.ts` | not supported | expected expression, on ` for (let i = 0; i < 10; ++i) { ` |
| `classes/classDeclarations/classExtendingBuiltinType.ts` | not supported | unknown type `Function`, on ` class C2 extends Function { } ` |
| `classes/classDeclarations/classExtendingClassLikeType.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `classes/classDeclarations/classExtendingNull.ts` | checks too little | 0 after the port |
| `classes/classDeclarations/classHeritageSpecification/classAppearsToHaveMembersOfObject.ts` | not supported | `as` to `C` is not yet supported: class types aren't yet supported as `as` targets, on ` let c: C = null as unknown as (C); ` |
| `classes/classDeclarations/classHeritageSpecification/classExtendingOptionalChain.ts` | not supported | expected `;` after expression, on ` namespace A { ` |
| `classes/classDeclarations/classHeritageSpecification/classExtendingPrimitive2.ts` | checks too little | 1 after the port |
| `classes/classDeclarations/classHeritageSpecification/classExtendsEveryObjectType2.ts` | not supported | tuple types must have at least one element, on ` class C6 extends []{ } // error ` |
| `classes/classDeclarations/classHeritageSpecification/classExtendsItself.ts` | checks too little | 3 after the port |
| `classes/classDeclarations/classHeritageSpecification/classExtendsItselfIndirectly2.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `classes/classDeclarations/classHeritageSpecification/classExtendsItselfIndirectly3.ts` | multi-file or JavaScript |  |
| `classes/classDeclarations/classHeritageSpecification/classExtendsShadowedConstructorFunction.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `classes/classDeclarations/classHeritageSpecification/classExtendsValidConstructorFunction.ts` | the port changes what it checks | `tsc` then reports TS7009 |
| `classes/classDeclarations/classHeritageSpecification/classIsSubtypeOfBaseType.ts` | checks too little | 1 after the port |
| `classes/classDeclarations/classHeritageSpecification/constructorFunctionTypeIsAssignableToBaseType.ts` | not supported | `any` is not supported, on ` bar: any; ` |
| `classes/classDeclarations/classHeritageSpecification/constructorFunctionTypeIsAssignableToBaseType2.ts` | not supported | `any` is not supported, on ` constructor(x: any) { ` |
| `classes/classDeclarations/classHeritageSpecification/derivedTypeDoesNotRequireExtendsClause.ts` | not supported | `as` to `Base` is not yet supported: class types aren't yet supported as `as` targets, on ` let b: Base = null as unknown as (Base); ` |
| `classes/classDeclarations/classImplementsMergedClassInterface.ts` | not supported | expected `;` after expression, on ` declare class C1 { ` |
| `classes/classDeclarations/classInsideBlock.ts` | checks too little | 0 after the port |
| `classes/classDeclarations/classWithPredefinedTypesAsNames.ts` | checks too little | 4 after the port |
| `classes/classDeclarations/classWithPredefinedTypesAsNames2.ts` | porter failure | still pruning after 40 passes; our first unsupported error: `void` is a reserved keyword and can't be used as a name |
| `classes/classDeclarations/classWithSemicolonClassElement1.ts` | checks too little | 0 after the port |
| `classes/classDeclarations/classWithSemicolonClassElement2.ts` | duplicate | of `classes/classDeclarations/classWithSemicolonClassElement1.ts` |
| `classes/classDeclarations/declaredClassMergedwithSelf.ts` | multi-file or JavaScript |  |
| `classes/classDeclarations/mergeClassInterfaceAndModule.ts` | not supported | expected `;` after expression, on ` declare class C1 {} ` |
| `classes/classDeclarations/mergedClassInterface.ts` | multi-file or JavaScript |  |
| `classes/classDeclarations/modifierOnClassDeclarationMemberInFunction.ts` | checks too little | 0 after the port |
| `classes/classExpression.ts` | not supported | expected expression, on ` let x = class C { ` |
| `classes/classExpressions/classExpression1.ts` | not supported | expected expression, on ` let v = class C {}; ` |
| `classes/classExpressions/classExpression2.ts` | not supported | expected expression, on ` let v = class C extends D {}; ` |
| `classes/classExpressions/classExpression3.ts` | not supported | expected expression, on ` let C = class extends class extends class { a: number = 1 } { b: number = 2 }... ` |
| `classes/classExpressions/classExpression4.ts` | the port changes what it checks | `tsc` then reports TS2749 |
| `classes/classExpressions/classExpression5.ts` | not supported | expected expression, on ` new class { ` |
| `classes/classExpressions/classExpressionLoop.ts` | not supported | expected expression, on ` for (let i = 0; i < 10; ++i) { ` |
| `classes/classExpressions/classWithStaticFieldInParameterBindingPattern.2.ts` | the port changes what it checks | `tsc` then reports TS2537, TS2448, TS2507, TS2322 |
| `classes/classExpressions/classWithStaticFieldInParameterBindingPattern.3.ts` | the port changes what it checks | `tsc` then reports TS2537, TS2373, TS2448, TS2507, TS2322 |
| `classes/classExpressions/classWithStaticFieldInParameterBindingPattern.ts` | the port changes what it checks | `tsc` then reports TS2537 |
| `classes/classExpressions/classWithStaticFieldInParameterInitializer.2.ts` | the port changes what it checks | `tsc` then reports TS2448, TS2507, TS2322 |
| `classes/classExpressions/classWithStaticFieldInParameterInitializer.3.ts` | the port changes what it checks | `tsc` then reports TS2373, TS2448, TS2507, TS2322 |
| `classes/classExpressions/classWithStaticFieldInParameterInitializer.ts` | not supported | default parameter values are only supported on function declarations, on ` ((b = class { static x: number = 1 }) => {})(); ` |
| `classes/classExpressions/extendClassExpressionFromModule.ts` | multi-file or JavaScript |  |
| `classes/classExpressions/genericClassExpressionInFunction.ts` | the port changes what it checks | `tsc` then reports TS1003, TS1005, TS2304, TS2339 |
| `classes/classExpressions/modifierOnClassExpressionMemberInFunction.ts` | not supported | expected expression, on ` let x = class C { ` |
| `classes/classStaticBlock/` | not supported | static blocks |
| `classes/constructorDeclarations/automaticConstructors/derivedClassWithoutExplicitConstructor2.ts` | not supported | optional function parameters are not yet supported, on ` constructor(x: number, y?: number, z?: number); ` |
| `classes/constructorDeclarations/classConstructorAccessibility.ts` | not supported | `protected` is not supported, on ` protected constructor(public x: number) { } ` |
| `classes/constructorDeclarations/classConstructorAccessibility3.ts` | not supported | `protected` is not supported, on ` protected constructor(public x: number) { } ` |
| `classes/constructorDeclarations/classConstructorAccessibility4.ts` | not supported | `protected` is not supported, on ` protected constructor() { } ` |
| `classes/constructorDeclarations/classConstructorAccessibility5.ts` | not supported | `protected` is not supported, on ` protected constructor() { } ` |
| `classes/constructorDeclarations/classConstructorOverloadsAccessibility.ts` | not supported | expected `{`, on ` protected constructor(a: number) // error ` |
| `classes/constructorDeclarations/classConstructorParametersAccessibility.ts` | not supported | parameter requires a type annotation, on ` constructor(protected p: number) { } ` |
| `classes/constructorDeclarations/classConstructorParametersAccessibility2.ts` | not supported | optional function parameters are not yet supported, on ` constructor(public x?: number) { } ` |
| `classes/constructorDeclarations/classConstructorParametersAccessibility3.ts` | not supported | parameter requires a type annotation, on ` constructor(protected p: number) { } ` |
| `classes/constructorDeclarations/classWithTwoConstructorDefinitions.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `classes/constructorDeclarations/constructorParameters/constructorDefaultValuesReferencingThis.ts` | not supported | parameter requires a type annotation, on ` constructor(x = this) { } ` |
| `classes/constructorDeclarations/constructorParameters/constructorImplementationWithDefaultValues.ts` | the port changes what it checks | `tsc` then reports TS7006, TS2322 |
| `classes/constructorDeclarations/constructorParameters/constructorImplementationWithDefaultValues2.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `classes/constructorDeclarations/constructorParameters/constructorOverloadsWithDefaultValues.ts` | not supported | expected `{`, on ` constructor(x: number = 1); // error ` |
| `classes/constructorDeclarations/constructorParameters/constructorOverloadsWithOptionalParameters.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `classes/constructorDeclarations/constructorParameters/constructorParameterProperties.ts` | not supported | parameter requires a type annotation, on ` constructor(private x: string, protected z: string) { } ` |
| `classes/constructorDeclarations/constructorParameters/constructorParameterProperties2.ts` | not supported | parameter requires a type annotation, on ` constructor(protected y: number) { } // error ` |
| `classes/constructorDeclarations/constructorParameters/declarationEmitReadonly.ts` | checks too little | 0 after the port |
| `classes/constructorDeclarations/constructorParameters/readonlyInAmbientClass.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `classes/constructorDeclarations/constructorParameters/readonlyReadonly.ts` | checks too little | 2 after the port |
| `classes/constructorDeclarations/constructorWithAssignableReturnExpression.ts` | checks too little | 4 after the port |
| `classes/constructorDeclarations/constructorWithExpressionLessReturn.ts` | checks too little | 0 after the port |
| `classes/constructorDeclarations/quotedConstructors.ts` | not supported | unknown escape sequence `\x`, on ` "\x63onstructor"() { ` |
| `classes/constructorDeclarations/superCalls/derivedClassSuperCallsInNonConstructorMembers.ts` | not supported | expected type, on ` a: super(); ` |
| `classes/constructorDeclarations/superCalls/derivedClassSuperCallsWithThisArg.ts` | the port changes what it checks | `tsc` then reports TS7006, TS2683 |
| `classes/constructorDeclarations/superCalls/derivedClassSuperProperties.ts` | the port changes what it checks | `tsc` then reports TS7006, TS2683, TS2551, TS7032, TS7034, TS7005 |
| `classes/constructorDeclarations/superCalls/emitStatementsBeforeSuperCallWithDefineFields.ts` | duplicate | of `classes/constructorDeclarations/superCalls/emitStatementsBeforeSuperCall.ts` |
| `classes/constructorDeclarations/superCalls/superCallInConstructorWithNoBaseType.ts` | checks too little | 4 after the port |
| `classes/constructorDeclarations/superCalls/superPropertyInConstructorBeforeSuperCall.ts` | not supported | optional function parameters are not yet supported, on ` constructor(x?: string) {} ` |
| `classes/indexMemberDeclarations/` | not supported | index signatures |
| `classes/members/accessibility/classPropertyAsPrivate.ts` | the port changes what it checks | `tsc` then reports TS2322, TS2721 |
| `classes/members/accessibility/classPropertyAsProtected.ts` | the port changes what it checks | `tsc` then reports TS2322, TS2721 |
| `classes/members/accessibility/classPropertyIsPublicByDefault.ts` | the port changes what it checks | `tsc` then reports TS2322, TS2721 |
| `classes/members/accessibility/privateClassPropertyAccessibleWithinClass.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `classes/members/accessibility/privateClassPropertyAccessibleWithinNestedClass.ts` | not supported | parameter requires a type annotation, on ` private set y(x) { this.y = this.x; } ` |
| `classes/members/accessibility/privateInstanceMemberAccessibility.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/members/accessibility/privateProtectedMembersAreNotAccessibleDestructuring.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/members/accessibility/privateStaticMemberAccessibility.ts` | checks too little | 3 after the port |
| `classes/members/accessibility/privateStaticNotAccessibleInClodule.ts` | not supported | expected `;` after expression, on ` namespace C { ` |
| `classes/members/accessibility/privateStaticNotAccessibleInClodule2.ts` | not supported | expected `;` after expression, on ` namespace D { ` |
| `classes/members/accessibility/protectedClassPropertyAccessibleWithinClass.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `classes/members/accessibility/protectedClassPropertyAccessibleWithinNestedClass.ts` | not supported | `protected` is not supported, on ` protected x: string; ` |
| `classes/members/accessibility/protectedClassPropertyAccessibleWithinNestedSubclass.ts` | not supported | `protected` is not supported, on ` protected x: string; ` |
| `classes/members/accessibility/protectedClassPropertyAccessibleWithinNestedSubclass1.ts` | not supported | `protected` is not supported, on ` protected x!: string; ` |
| `classes/members/accessibility/protectedClassPropertyAccessibleWithinSubclass.ts` | not supported | `protected` is not supported, on ` protected x: string; ` |
| `classes/members/accessibility/protectedClassPropertyAccessibleWithinSubclass2.ts` | not supported | `protected` is not supported, on ` protected x!: string; ` |
| `classes/members/accessibility/protectedClassPropertyAccessibleWithinSubclass3.ts` | not supported | `protected` is not supported, on ` protected x: string; ` |
| `classes/members/accessibility/protectedInstanceMemberAccessibility.ts` | not supported | `protected` is not supported, on ` protected x!: string; ` |
| `classes/members/accessibility/protectedStaticClassPropertyAccessibleWithinSubclass.ts` | not supported | `protected` is not supported, on ` protected static x: string; ` |
| `classes/members/accessibility/protectedStaticClassPropertyAccessibleWithinSubclass2.ts` | not supported | `protected` is not supported, on ` protected static x: string; ` |
| `classes/members/accessibility/protectedStaticNotAccessibleInClodule.ts` | not supported | `protected` is not supported, on ` protected static bar: string; ` |
| `classes/members/classTypes/genericSetterInClassType.ts` | not supported | unexpected character `#`, on ` #value!: T; ` |
| `classes/members/classTypes/genericSetterInClassTypeJsDoc.ts` | multi-file or JavaScript |  |
| `classes/members/classTypes/indexersInClassType.ts` | not supported | expected class member name, on ` [x: number]: Date; ` |
| `classes/members/classTypes/instancePropertiesInheritedIntoClassType.ts` | not supported | expected `;` after expression, on ` namespace NonGeneric { ` |
| `classes/members/classTypes/instancePropertyInClassType.ts` | not supported | expected `;` after expression, on ` namespace NonGeneric { ` |
| `classes/members/classTypes/staticPropertyNotInClassType.ts` | not supported | expected `;` after expression, on ` namespace NonGeneric { ` |
| `classes/members/constructorFunctionTypes/classWithConstructors.ts` | not supported | expected `;` after expression, on ` namespace NonGeneric { ` |
| `classes/members/constructorFunctionTypes/classWithStaticMembers.ts` | not supported | static accessors are not supported, on ` static get x() { return 1; } ` |
| `classes/members/constructorFunctionTypes/constructorHasPrototypeProperty.ts` | not supported | expected `;` after expression, on ` namespace NonGeneric { ` |
| `classes/members/inheritanceAndOverriding/derivedClassFunctionOverridesBaseClassAccessor.ts` | not supported | parameter requires a type annotation, on ` set x(v) { ` |
| `classes/members/inheritanceAndOverriding/derivedClassIncludesInheritedMembers.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `classes/members/inheritanceAndOverriding/derivedClassOverridesIndexersWithAssignmentCompatibility.ts` | not supported | index signatures |
| `classes/members/inheritanceAndOverriding/derivedClassOverridesPrivates.ts` | checks too little | 2 after the port |
| `classes/members/inheritanceAndOverriding/derivedClassOverridesProtectedMembers.ts` | not supported | `protected` is not supported, on ` protected a: typeof x; ` |
| `classes/members/inheritanceAndOverriding/derivedClassOverridesProtectedMembers2.ts` | not supported | `protected` is not supported, on ` protected a: typeof x; ` |
| `classes/members/inheritanceAndOverriding/derivedClassOverridesProtectedMembers3.ts` | not supported | static accessors are not supported, on ` static get t() { return x; } ` |
| `classes/members/inheritanceAndOverriding/derivedClassOverridesProtectedMembers4.ts` | not supported | `protected` is not supported, on ` protected a: typeof x; ` |
| `classes/members/inheritanceAndOverriding/derivedClassOverridesPublicMembers.ts` | not supported | static accessors are not supported, on ` static get t() { return x; } ` |
| `classes/members/inheritanceAndOverriding/derivedClassOverridesWithoutSubtype.ts` | not supported | `any` is not supported, on ` foo: any; ` |
| `classes/members/inheritanceAndOverriding/derivedClassTransitivity.ts` | not supported | optional function parameters are not yet supported, on ` foo(x?: string): void { } // ok to add optional parameters ` |
| `classes/members/inheritanceAndOverriding/derivedClassTransitivity2.ts` | not supported | optional function parameters are not yet supported, on ` foo(x: number, y?: string): void { } // ok to add optional parameters ` |
| `classes/members/inheritanceAndOverriding/derivedClassTransitivity3.ts` | not supported | optional function parameters are not yet supported, on ` foo(x: T, y?: number): void { } // ok to add optional parameters ` |
| `classes/members/inheritanceAndOverriding/derivedClassTransitivity4.ts` | not supported | `protected` is not supported, on ` protected foo(x: number): void { } ` |
| `classes/members/inheritanceAndOverriding/derivedClassWithAny.ts` | not supported | static accessors are not supported, on ` static get Y(): number { ` |
| `classes/members/inheritanceAndOverriding/derivedClassWithPrivateInstanceShadowingProtectedInstance.ts` | not supported | `protected` is not supported, on ` protected x: string; ` |
| `classes/members/inheritanceAndOverriding/derivedClassWithPrivateInstanceShadowingPublicInstance.ts` | not supported | parameter requires a type annotation, on ` public set a(v) { } ` |
| `classes/members/inheritanceAndOverriding/derivedClassWithPrivateStaticShadowingProtectedStatic.ts` | not supported | `protected` is not supported, on ` protected static x: string; ` |
| `classes/members/inheritanceAndOverriding/derivedClassWithPrivateStaticShadowingPublicStatic.ts` | not supported | static accessors are not supported, on ` public static get a() { return 1; } ` |
| `classes/members/inheritanceAndOverriding/derivedGenericClassWithAny.ts` | not supported | expected `,` or `>`, on ` class C<T extends number> { ` |
| `classes/members/instanceAndStaticMembers/superInStaticMembers1.ts` | multi-file or JavaScript |  |
| `classes/members/instanceAndStaticMembers/thisAndSuperInStaticMembers1.ts` | not supported | expected `;` after expression, on ` declare class B { ` |
| `classes/members/instanceAndStaticMembers/thisAndSuperInStaticMembers2.ts` | not supported | expected `;` after expression, on ` declare class B { ` |
| `classes/members/instanceAndStaticMembers/thisAndSuperInStaticMembers3.ts` | not supported | expected `;` after expression, on ` declare class B { ` |
| `classes/members/instanceAndStaticMembers/thisAndSuperInStaticMembers4.ts` | not supported | expected `;` after expression, on ` declare class B { ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInInstanceMember.ts` | not supported | class fields require a type annotation, on ` x = this; ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInInstanceMember2.ts` | not supported | class fields require a type annotation, on ` x = this; ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInstanceMemberNarrowedWithLoopAntecedent.ts` | not supported | expected `:` and a type for the class field, on ` state!: State; ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers10.ts` | not supported | unexpected character `@`, on ` @foo ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers11.ts` | not supported | unexpected character `@`, on ` @foo ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers12.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers13.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers2.ts` | checks too little | 0 after the port |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers4.ts` | duplicate | of `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers3.ts` |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers5.ts` | checks too little | 1 after the port |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers6.ts` | checks too little | 3 after the port |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers8.ts` | not supported | class fields require a type annotation, on ` static functionExprBoundary = function () { return this.f + 2 }; ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers9.ts` | not supported | class fields require a type annotation, on ` static functionExprBoundary = function () { return super.f + 2 }; ` |
| `classes/members/privateNames/privateNameAccessors.ts` | not supported | unexpected character `#`, on ` get #prop() { return ""; } ` |
| `classes/members/privateNames/privateNameAccessorsAccess.ts` | not supported | unexpected character `#`, on ` get #prop() { return ""; } ` |
| `classes/members/privateNames/privateNameAccessorsCallExpression.ts` | the port changes what it checks | `tsc` then reports TS2683, TS7006, TS7019, TS7009 |
| `classes/members/privateNames/privateNameAccessorssDerivedClasses.ts` | not supported | unexpected character `#`, on ` get #prop(): number { return  123; } ` |
| `classes/members/privateNames/privateNameAmbientNoImplicitAny.ts` | not supported | unexpected character `#`, on ` #prop; ` |
| `classes/members/privateNames/privateNameAndAny.ts` | not supported | unexpected character `#`, on ` #foo = true; ` |
| `classes/members/privateNames/privateNameAndIndexSignature.ts` | not supported | index signatures |
| `classes/members/privateNames/privateNameAndObjectRestSpread.ts` | not supported | unexpected character `#`, on ` #prop = 1; ` |
| `classes/members/privateNames/privateNameAndPropertySignature.ts` | not supported | unexpected character `#`, on ` #foo: string; ` |
| `classes/members/privateNames/privateNameAndStaticInitializer.ts` | not supported | unexpected character `#`, on ` #foo = 1; ` |
| `classes/members/privateNames/privateNameBadAssignment.ts` | not supported | unexpected character `#`, on ` exports.#nope = 1;           // Error (outside class body) ` |
| `classes/members/privateNames/privateNameBadDeclaration.ts` | not supported | unexpected character `#`, on ` #x: 1,         // Error ` |
| `classes/members/privateNames/privateNameBadSuper.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/members/privateNames/privateNameBadSuperUseDefineForClassFields.ts` | the port changes what it checks | `tsc` then reports TS7008, TS2376 |
| `classes/members/privateNames/privateNameCircularReference.ts` | not supported | unexpected character `#`, on ` #foo = this.#bar; ` |
| `classes/members/privateNames/privateNameClassExpressionLoop.ts` | not supported | unexpected character `#`, on ` #myField = "hello"; ` |
| `classes/members/privateNames/privateNameComputedPropertyName1.ts` | not supported | unexpected character `#`, on ` #a = 'a'; ` |
| `classes/members/privateNames/privateNameComputedPropertyName2.ts` | not supported | unexpected character `#`, on ` #x = 100; ` |
| `classes/members/privateNames/privateNameComputedPropertyName3.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7053 |
| `classes/members/privateNames/privateNameComputedPropertyName4.ts` | not supported | unexpected character `#`, on ` static #qux = 42; ` |
| `classes/members/privateNames/privateNameConstructorReserved.ts` | not supported | unexpected character `#`, on `` #constructor(): void {}      // Error: `#constructor` is a reserved word. `` |
| `classes/members/privateNames/privateNameConstructorSignature.ts` | not supported | construct signatures |
| `classes/members/privateNames/privateNameDeclaration.ts` | not supported | unexpected character `#`, on ` #foo: string; ` |
| `classes/members/privateNames/privateNameDeclarationMerging.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `classes/members/privateNames/privateNameDuplicateField.ts` | not supported | unexpected character `#`, on ` #foo = "foo"; ` |
| `classes/members/privateNames/privateNameEmitHelpers.ts` | multi-file or JavaScript |  |
| `classes/members/privateNames/privateNameEnum.ts` | not supported | unexpected character `#`, on ` #x ` |
| `classes/members/privateNames/privateNameES5Ban.ts` | not supported | unexpected character `#`, on ` #field = 123; ` |
| `classes/members/privateNames/privateNameField.ts` | not supported | unexpected character `#`, on ` #name: string; ` |
| `classes/members/privateNames/privateNameFieldAccess.ts` | not supported | unexpected character `#`, on ` #myField = "hello world"; ` |
| `classes/members/privateNames/privateNameFieldAssignment.ts` | not supported | unexpected character `#`, on ` #field = 0; ` |
| `classes/members/privateNames/privateNameFieldCallExpression.ts` | the port changes what it checks | `tsc` then reports TS2683, TS7006, TS7019, TS7009 |
| `classes/members/privateNames/privateNameFieldClassExpression.ts` | not supported | unexpected character `#`, on ` #foo = class { ` |
| `classes/members/privateNames/privateNameFieldDerivedClasses.ts` | not supported | unexpected character `#`, on ` #prop: number = 123; ` |
| `classes/members/privateNames/privateNameFieldDestructuredBinding.ts` | not supported | unexpected character `#`, on ` #field = 1; ` |
| `classes/members/privateNames/privateNameFieldInitializer.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/members/privateNames/privateNameFieldParenthesisLeftAssignment.ts` | not supported | unexpected character `#`, on ` #p: number; ` |
| `classes/members/privateNames/privateNameFieldsESNext.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/members/privateNames/privateNameFieldUnaryMutation.ts` | not supported | unexpected character `#`, on ` #test: number = 24; ` |
| `classes/members/privateNames/privateNameHashCharName.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/members/privateNames/privateNameImplicitDeclaration.ts` | multi-file or JavaScript |  |
| `classes/members/privateNames/privateNameInInExpression.ts` | not supported | unexpected character `#`, on ` #field = 1; ` |
| `classes/members/privateNames/privateNameInInExpressionTransform.ts` | not supported | unexpected character `#`, on ` #field = 1; ` |
| `classes/members/privateNames/privateNameInInExpressionUnused.ts` | not supported | unexpected character `#`, on ` #unused: null; // expect unused error ` |
| `classes/members/privateNames/privateNameInLhsReceiverExpression.ts` | not supported | unexpected character `#`, on ` #y = 123; ` |
| `classes/members/privateNames/privateNameInObjectLiteral-1.ts` | not supported | unexpected character `#`, on ` #foo: 1 ` |
| `classes/members/privateNames/privateNameInObjectLiteral-2.ts` | not supported | unexpected character `#`, on ` #foo() { ` |
| `classes/members/privateNames/privateNameInObjectLiteral-3.ts` | not supported | unexpected character `#`, on ` get #foo() { ` |
| `classes/members/privateNames/privateNameJsBadAssignment.ts` | multi-file or JavaScript |  |
| `classes/members/privateNames/privateNameJsBadDeclaration.ts` | multi-file or JavaScript |  |
| `classes/members/privateNames/privateNameLateSuper.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/members/privateNames/privateNameLateSuperUseDefineForClassFields.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/members/privateNames/privateNameMethod.ts` | not supported | unexpected character `#`, on ` #method(param: string): string { ` |
| `classes/members/privateNames/privateNameMethodAccess.ts` | not supported | unexpected character `#`, on ` #method(): string { return "" } ` |
| `classes/members/privateNames/privateNameMethodAssignment.ts` | not supported | unexpected character `#`, on ` #method(): void { }; ` |
| `classes/members/privateNames/privateNameMethodAsync.ts` | not supported | unexpected character `#`, on ` async #bar(): Promise<number> { return await Promise.resolve(42); } ` |
| `classes/members/privateNames/privateNameMethodCallExpression.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7019, TS7009 |
| `classes/members/privateNames/privateNameMethodClassExpression.ts` | the port changes what it checks | `tsc` then reports TS2749, TS18016 |
| `classes/members/privateNames/privateNameMethodInStaticFieldInit.ts` | not supported | unexpected character `#`, on ` static s: number = new C().#method(); ` |
| `classes/members/privateNames/privateNameMethodsDerivedClasses.ts` | not supported | unexpected character `#`, on ` #prop(): number{ return  123; } ` |
| `classes/members/privateNames/privateNameNestedClassAccessorsShadowing.ts` | not supported | unexpected character `#`, on ` get #x() { return 1; }; ` |
| `classes/members/privateNames/privateNameNestedClassFieldShadowing.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/members/privateNames/privateNameNestedClassMethodShadowing.ts` | not supported | unexpected character `#`, on ` #x(): void { }; ` |
| `classes/members/privateNames/privateNameNestedClassNameConflict.ts` | not supported | unexpected character `#`, on ` #foo: string; ` |
| `classes/members/privateNames/privateNameNestedMethodAccess.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `classes/members/privateNames/privateNameNotAccessibleOutsideDefiningClass.ts` | not supported | unexpected character `#`, on ` #foo: number = 3; ` |
| `classes/members/privateNames/privateNameNotAllowedOutsideClass.ts` | not supported | unexpected character `#`, on ` const #foo = 3; ` |
| `classes/members/privateNames/privateNameReadonly.ts` | not supported | unexpected character `#`, on ` #bar(): void {} ` |
| `classes/members/privateNames/privateNamesAndDecorators.ts` | not supported | unexpected character `@`, on ` @dec                // Error ` |
| `classes/members/privateNames/privateNamesAndFields.ts` | not supported | unexpected character `#`, on ` #foo: number; ` |
| `classes/members/privateNames/privateNamesAndGenericClasses-2.ts` | not supported | unexpected character `#`, on ` #foo: T; ` |
| `classes/members/privateNames/privateNamesAndIndexedAccess.ts` | not supported | unexpected character `#`, on ` #bar = 3; ` |
| `classes/members/privateNames/privateNamesAndkeyof.ts` | not supported | unexpected character `#`, on ` #fooField = 3; ` |
| `classes/members/privateNames/privateNamesAndMethods.ts` | not supported | unexpected character `#`, on ` #foo(a: number): void {} ` |
| `classes/members/privateNames/privateNamesAndStaticFields.ts` | not supported | unexpected character `#`, on ` static #foo: number; ` |
| `classes/members/privateNames/privateNamesAndStaticMethods.ts` | not supported | unexpected character `#`, on ` static #foo(a: number): void {} ` |
| `classes/members/privateNames/privateNamesAssertion.ts` | not supported | unexpected character `#`, on ` #p1: (v: any) => asserts v is string = (v) => { ` |
| `classes/members/privateNames/privateNamesConstructorChain-1.ts` | not supported | unexpected character `#`, on ` #foo = 3; ` |
| `classes/members/privateNames/privateNamesConstructorChain-2.ts` | not supported | unexpected character `#`, on ` #foo = 3; ` |
| `classes/members/privateNames/privateNameSetterExprReturnValue.ts` | not supported | unexpected character `#`, on ` set #foo(a: number) {} ` |
| `classes/members/privateNames/privateNameSetterNoGetter.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `classes/members/privateNames/privateNamesIncompatibleModifiers.ts` | not supported | unexpected character `#`, on ` public #foo = 3;         // Error ` |
| `classes/members/privateNames/privateNamesIncompatibleModifiersJs.ts` | multi-file or JavaScript |  |
| `classes/members/privateNames/privateNamesInGenericClasses.ts` | not supported | unexpected character `#`, on ` #foo: T; ` |
| `classes/members/privateNames/privateNamesInNestedClasses-1.ts` | not supported | unexpected character `#`, on ` #foo = "A's #foo"; ` |
| `classes/members/privateNames/privateNamesInNestedClasses-2.ts` | not supported | unexpected character `#`, on ` static #x = 5; ` |
| `classes/members/privateNames/privateNamesInterfaceExtendingClass.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/members/privateNames/privateNamesNoDelete.ts` | not supported | `delete` |
| `classes/members/privateNames/privateNamesNotAllowedAsParameters.ts` | not supported | unexpected character `#`, on ` setFoo(#foo: string): void {} ` |
| `classes/members/privateNames/privateNamesNotAllowedInVariableDeclarations.ts` | not supported | unexpected character `#`, on ` const #foo = 3; ` |
| `classes/members/privateNames/privateNameStaticAccessors.ts` | not supported | unexpected character `#`, on ` static get #prop() { return ""; } ` |
| `classes/members/privateNames/privateNameStaticAccessorsAccess.ts` | not supported | unexpected character `#`, on ` static get #prop() { return ""; } ` |
| `classes/members/privateNames/privateNameStaticAccessorsCallExpression.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7019, TS7009 |
| `classes/members/privateNames/privateNameStaticAccessorssDerivedClasses.ts` | not supported | unexpected character `#`, on ` static get #prop(): number { return  123; } ` |
| `classes/members/privateNames/privateNameStaticAndStaticInitializer.ts` | not supported | unexpected character `#`, on ` static #foo = 1; ` |
| `classes/members/privateNames/privateNameStaticEmitHelpers.ts` | multi-file or JavaScript |  |
| `classes/members/privateNames/privateNameStaticFieldAccess.ts` | not supported | unexpected character `#`, on ` static #myField = "hello world"; ` |
| `classes/members/privateNames/privateNameStaticFieldAssignment.ts` | not supported | unexpected character `#`, on ` static #field = 0; ` |
| `classes/members/privateNames/privateNameStaticFieldCallExpression.ts` | the port changes what it checks | `tsc` then reports TS2683, TS7006, TS7019, TS7009 |
| `classes/members/privateNames/privateNameStaticFieldClassExpression.ts` | not supported | unexpected character `#`, on ` static #foo = class { ` |
| `classes/members/privateNames/privateNameStaticFieldDerivedClasses.ts` | not supported | unexpected character `#`, on ` static #prop: number = 123; ` |
| `classes/members/privateNames/privateNameStaticFieldDestructuredBinding.ts` | not supported | unexpected character `#`, on ` static #field = 1; ` |
| `classes/members/privateNames/privateNameStaticFieldInitializer.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/members/privateNames/privateNameStaticFieldNoInitializer.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/members/privateNames/privateNameStaticFieldUnaryMutation.ts` | not supported | unexpected character `#`, on ` static #test: number = 24; ` |
| `classes/members/privateNames/privateNameStaticMethod.ts` | not supported | unexpected character `#`, on ` static #method(param: string): string { ` |
| `classes/members/privateNames/privateNameStaticMethodAssignment.ts` | not supported | unexpected character `#`, on ` static #method(): void { }; ` |
| `classes/members/privateNames/privateNameStaticMethodAsync.ts` | not supported | unexpected character `#`, on ` static async #bar(): Promise<number> { return await Promise.resolve(42); } ` |
| `classes/members/privateNames/privateNameStaticMethodCallExpression.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7019, TS7009 |
| `classes/members/privateNames/privateNameStaticMethodClassExpression.ts` | not supported | unexpected character `#`, on ` static #field = D.#method(); ` |
| `classes/members/privateNames/privateNameStaticMethodInStaticFieldInit.ts` | not supported | unexpected character `#`, on ` static s: number = C.#method(); ` |
| `classes/members/privateNames/privateNameStaticsAndStaticMethods.ts` | not supported | unexpected character `#`, on ` static #foo(a: number): void {} ` |
| `classes/members/privateNames/privateNamesUnique-1.ts` | not supported | unexpected character `#`, on ` #foo: number; ` |
| `classes/members/privateNames/privateNamesUnique-2.ts` | multi-file or JavaScript |  |
| `classes/members/privateNames/privateNamesUnique-3.ts` | not supported | unexpected character `#`, on ` #foo = 1; ` |
| `classes/members/privateNames/privateNamesUnique-4.ts` | not supported | unexpected character `#`, on ` class C { #something: number } ` |
| `classes/members/privateNames/privateNamesUnique-5.ts` | not supported | unexpected character `#`, on ` #foo: number; ` |
| `classes/members/privateNames/privateNamesUseBeforeDef.ts` | the port changes what it checks | `tsc` then reports TS7022 |
| `classes/members/privateNames/privateNameUncheckedJsOptionalChain.ts` | multi-file or JavaScript |  |
| `classes/members/privateNames/privateNameUnused.ts` | not supported | unexpected character `#`, on ` #used = "used"; ` |
| `classes/members/privateNames/privateNameWhenNotUseDefineForClassFieldsInEsNext.ts` | the port changes what it checks | `tsc` then reports TS1003, TS1005, TS1128, TS2348, TS2304 |
| `classes/members/privateNames/privateStaticNameShadowing.ts` | not supported | unexpected character `#`, on ` static #f = X.#m(); ` |
| `classes/members/privateNames/privateWriteOnlyAccessorRead.ts` | not supported | unexpected character `#`, on ` set #value(v: { foo: { bar: number } }) {} ` |
| `classes/members/privateNames/typeFromPrivatePropertyAssignment.ts` | not supported | unexpected character `#`, on ` #a?: Foo; ` |
| `classes/members/privateNames/typeFromPrivatePropertyAssignmentJs.ts` | multi-file or JavaScript |  |
| `classes/methodDeclarations/optionalMethodDeclarations.ts` | not supported | expected `:` and a type for the class field, on ` method?(): void {} ` |
| `classes/mixinAbstractClasses.2.ts` | not supported | unexpected character `&`, on ` function Mixin<TBaseClass extends abstract new (...args: any) => any>(baseCla... ` |
| `classes/mixinAbstractClasses.ts` | not supported | unexpected character `&`, on ` function Mixin<TBaseClass extends abstract new (...args: any) => any>(baseCla... ` |
| `classes/mixinAbstractClassesReturnTypeInference.ts` | the port changes what it checks | `tsc` then reports TS1005, TS2304, TS2749, TS7008, TS2322 |
| `classes/mixinAccessModifiers.ts` | not supported | unexpected character `&`, on ` function f1(x: Private & Private2): void { ` |
| `classes/mixinAccessors1.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `classes/mixinAccessors2.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `classes/mixinAccessors3.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `classes/mixinAccessors4.ts` | the port changes what it checks | `tsc` then reports TS1005, TS2304, TS2749, TS7008, TS2322 |
| `classes/mixinAccessors5.ts` | not supported | unexpected character `&`, on ` ): T & U { return null as unknown as (T & U); } ` |
| `classes/mixinClassesAnnotated.ts` | not supported | unexpected character `&`, on ` const Printable = <T extends Constructor<Base>>(superClass: T): Constructor<P... ` |
| `classes/mixinClassesAnonymous.ts` | the port changes what it checks | `tsc` then reports TS1005, TS2304, TS2749, TS7008, TS2322, TS2339 |
| `classes/mixinClassesMembers.ts` | not supported | unexpected character `&`, on ` const Mixed1: typeof M1 & typeof C1 = null as unknown as (typeof M1 & typeof ... ` |
| `classes/mixinWithBaseDependingOnSelfNoCrash1.ts` | not supported | expected `;` after expression, on ` declare class Document<Parent> {} ` |
| `classes/nestedClassDeclaration.ts` | porter failure | nothing to prune at offsets 181; our first unsupported error: expected `:` and a type for the class field |
| `classes/propertyMemberDeclarations/abstractProperty.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/propertyMemberDeclarations/abstractPropertyInitializer.ts` | not supported | expected `;` after expression, on ` abstract class C { ` |
| `classes/propertyMemberDeclarations/accessibilityModifiers.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/propertyMemberDeclarations/accessorsOverrideMethod.ts` | checks too little | 2 after the port |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty.ts` | not supported | parameter requires a type annotation, on ` set p(value) { this._secret = value } // error ` |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty10.ts` | checks too little | 1 after the port |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty2.ts` | not supported | parameter requires a type annotation, on `` set x(value) { console.log(`x was set to ${value}`); } `` |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty3.ts` | not supported | expected `;` after expression, on ` declare class Animal { ` |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty4.ts` | not supported | expected `;` after expression, on ` declare class Animal { ` |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty5.ts` | not supported | parameter requires a type annotation, on ` set p(value) { } ` |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty6.ts` | duplicate | of `classes/propertyMemberDeclarations/accessorsOverrideProperty.ts` |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty7.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty8.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty9.ts` | not supported | unexpected character `&`, on ` ): TBaseClass & (new (...args: any[]) => ApiItemContainerMixin) { ` |
| `classes/propertyMemberDeclarations/assignParameterPropertyToPropertyDeclarationES2022.ts` | not supported | class fields require a type annotation, on ` Inner = class extends F { ` |
| `classes/propertyMemberDeclarations/assignParameterPropertyToPropertyDeclarationESNext.ts` | not supported | class fields require a type annotation, on ` Inner = class extends F { ` |
| `classes/propertyMemberDeclarations/autoAccessor1.ts` | not supported | expected `:` and a type for the class field, on ` accessor a: any; ` |
| `classes/propertyMemberDeclarations/autoAccessor10.ts` | not supported | unexpected character `#`, on ` #a1_accessor_storage = 1; ` |
| `classes/propertyMemberDeclarations/autoAccessor11.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/propertyMemberDeclarations/autoAccessor2.ts` | not supported | unexpected character `#`, on ` accessor #a: any; ` |
| `classes/propertyMemberDeclarations/autoAccessor3.ts` | not supported | expected `:` and a type for the class field, on ` accessor "w": any; ` |
| `classes/propertyMemberDeclarations/autoAccessor4.ts` | not supported | expected `:` and a type for the class field, on ` accessor 0: any; ` |
| `classes/propertyMemberDeclarations/autoAccessor5.ts` | not supported | expected `:` and a type for the class field, on ` accessor ["w"]: any; ` |
| `classes/propertyMemberDeclarations/autoAccessor6.ts` | not supported | expected `:` and a type for the class field, on ` accessor a: any; ` |
| `classes/propertyMemberDeclarations/autoAccessor7.ts` | not supported | expected `;` after expression, on ` abstract class C1 { ` |
| `classes/propertyMemberDeclarations/autoAccessor8.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `classes/propertyMemberDeclarations/autoAccessor9.ts` | not supported | unexpected character `#`, on ` #x = 1; ` |
| `classes/propertyMemberDeclarations/autoAccessorAllowedModifiers.ts` | not supported | unexpected character `#`, on ` accessor #j: any; ` |
| `classes/propertyMemberDeclarations/autoAccessorDisallowedModifiers.ts` | the port changes what it checks | `tsc` then reports TS2792 |
| `classes/propertyMemberDeclarations/autoAccessorExperimentalDecorators.ts` | the port changes what it checks | `tsc` then reports TS1240 |
| `classes/propertyMemberDeclarations/autoAccessorNoUseDefineForClassFields.ts` | multi-file or JavaScript |  |
| `classes/propertyMemberDeclarations/canFollowGetSetKeyword.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/propertyMemberDeclarations/constructorParameterShadowsOuterScopes.ts` | not supported | class fields require a type annotation, on ` b = x; // error, evaluated in scope of constructor, cannot reference x ` |
| `classes/propertyMemberDeclarations/constructorParameterShadowsOuterScopes2.ts` | the port changes what it checks | `tsc` then reports TS2301 |
| `classes/propertyMemberDeclarations/defineProperty.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/propertyMemberDeclarations/derivedUninitializedPropertyDeclaration.ts` | not supported | `any` is not supported, on ` property: any; // error ` |
| `classes/propertyMemberDeclarations/initializerReferencingConstructorLocals.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `classes/propertyMemberDeclarations/initializerReferencingConstructorParameters.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `classes/propertyMemberDeclarations/instanceMemberInitialization.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `classes/propertyMemberDeclarations/instanceMemberWithComputedPropertyName.ts` | not supported | expected class member name, on ` [x] = true; ` |
| `classes/propertyMemberDeclarations/instanceMemberWithComputedPropertyName2.ts` | not supported | expected class member name, on ` [x]: string; ` |
| `classes/propertyMemberDeclarations/memberAccessorDeclarations/accessorsAreNotContextuallyTyped.ts` | not supported | `as` to `C` is not yet supported: class types aren't yet supported as `as` targets, on ` let c: C = null as unknown as (C); ` |
| `classes/propertyMemberDeclarations/memberAccessorDeclarations/accessorWithES5.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `classes/propertyMemberDeclarations/memberAccessorDeclarations/accessorWithMismatchedAccessibilityModifiers.ts` | not supported | parameter requires a type annotation, on ` private set x(v) { ` |
| `classes/propertyMemberDeclarations/memberAccessorDeclarations/ambientAccessors.ts` | not supported | expected `;` after expression, on ` declare class C { ` |
| `classes/propertyMemberDeclarations/memberAccessorDeclarations/typeOfThisInAccessor.ts` | not supported | static accessors are not supported, on ` static get y() { ` |
| `classes/propertyMemberDeclarations/memberFunctionDeclarations/instanceMemberAssignsToClassPrototype.ts` | not supported | parameter `x` requires a type annotation, on ` C.prototype.bar = (x) => x; // ok ` |
| `classes/propertyMemberDeclarations/memberFunctionDeclarations/memberFunctionOverloadMixingStaticAndInstance.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `classes/propertyMemberDeclarations/memberFunctionDeclarations/memberFunctionsWithPrivateOverloads.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `classes/propertyMemberDeclarations/memberFunctionDeclarations/memberFunctionsWithPublicOverloads.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `classes/propertyMemberDeclarations/memberFunctionDeclarations/memberFunctionsWithPublicPrivateOverloads.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `classes/propertyMemberDeclarations/memberFunctionDeclarations/staticFactory1.ts` | checks too little | 4 after the port |
| `classes/propertyMemberDeclarations/memberFunctionDeclarations/typeOfThisInMemberFunctions.ts` | not supported | expected `,` or `>`, on ` class E<T extends Date> { ` |
| `classes/propertyMemberDeclarations/optionalMethod.ts` | not supported | expected `:` and a type for the class field, on ` method?(): void { } ` |
| `classes/propertyMemberDeclarations/optionalProperty.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `classes/propertyMemberDeclarations/overrideInterfaceProperty.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `classes/propertyMemberDeclarations/propertyAndAccessorWithSameName.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `classes/propertyMemberDeclarations/propertyAndFunctionWithSameName.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `classes/propertyMemberDeclarations/propertyNamedConstructor.ts` | not supported | class fields require a type annotation, on ` "constructor" = 3; // Error ` |
| `classes/propertyMemberDeclarations/propertyNamedPrototype.ts` | checks too little | 1 after the port |
| `classes/propertyMemberDeclarations/propertyOverridesAccessors.ts` | not supported | parameter requires a type annotation, on ` set p(value) { this._secret = value } ` |
| `classes/propertyMemberDeclarations/propertyOverridesAccessors2.ts` | not supported | parameter requires a type annotation, on `` set x(value) { console.log(`x was set to ${value}`); } `` |
| `classes/propertyMemberDeclarations/propertyOverridesAccessors3.ts` | not supported | parameter requires a type annotation, on ` set sound(val) { ` |
| `classes/propertyMemberDeclarations/propertyOverridesAccessors4.ts` | not supported | expected `;` after expression, on ` declare class Animal { ` |
| `classes/propertyMemberDeclarations/propertyOverridesAccessors5.ts` | checks too little | 2 after the port |
| `classes/propertyMemberDeclarations/propertyOverridesAccessors6.ts` | checks too little | 1 after the port |
| `classes/propertyMemberDeclarations/propertyOverridesMethod.ts` | checks too little | 1 after the port |
| `classes/propertyMemberDeclarations/redeclaredProperty.ts` | checks too little | 4 after the port |
| `classes/propertyMemberDeclarations/redefinedPararameterProperty.ts` | checks too little | 3 after the port |
| `classes/propertyMemberDeclarations/staticAndNonStaticPropertiesSameName.ts` | checks too little | 0 after the port |
| `classes/propertyMemberDeclarations/staticAutoAccessors.ts` | not supported | expected `:` and a type for the class field, on ` static accessor x: number = 1; ` |
| `classes/propertyMemberDeclarations/staticAutoAccessorsWithDecorators.ts` | not supported | unexpected character `@`, on ` @((t, c) => {}) ` |
| `classes/propertyMemberDeclarations/staticPropertyAndFunctionWithSameName.ts` | checks too little | 0 after the port |
| `classes/propertyMemberDeclarations/staticPropertyNameConflicts.ts` | not supported | expected type, on ` } as const; ` |
| `classes/propertyMemberDeclarations/staticPropertyNameConflictsInAmbientContext.ts` | multi-file or JavaScript |  |
| `classes/propertyMemberDeclarations/strictPropertyInitialization.ts` | not supported | unexpected character `#`, on ` #f: number; //Error ` |
| `classes/propertyMemberDeclarations/thisInInstanceMemberInitializer.ts` | not supported | class fields require a type annotation, on ` x = this; ` |
| `classes/propertyMemberDeclarations/thisPropertyOverridesAccessors.ts` | multi-file or JavaScript |  |
| `classes/propertyMemberDeclarations/twoAccessorsWithSameName.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `classes/propertyMemberDeclarations/twoAccessorsWithSameName2.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `classes/staticIndexSignature/` | not supported | index signatures |
| `constEnums/constEnum1.ts` | not supported | unexpected character `~`, on ` d = ~e, ` |
| `constEnums/constEnum2.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum D { ` |
| `constEnums/constEnum3.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum TestType { foo, bar } ` |
| `constEnums/constEnum4.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum A { } ` |
| `constEnums/constEnumNoObjectPrototypePropertyAccess.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum Bebra {} ` |
| `constEnums/constEnumPropertyAccess1.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum G { ` |
| `constEnums/constEnumPropertyAccess2.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum G { ` |
| `constEnums/constEnumPropertyAccess3.ts` | not supported | unexpected character `~`, on ` A = ~1, ` |
| `constEnums/importElisionConstEnumMerge1.ts` | multi-file or JavaScript |  |
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
| `declarationEmit/anonymousClassAccessorsDeclarationEmit1.ts` | the port changes what it checks | `tsc` then reports TS1005, TS2552, TS2304, TS2749, TS7008, TS2322 |
| `declarationEmit/classDoesNotDependOnPrivateMember.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `declarationEmit/declarationEmitWorkWithInlineComments.ts` | checks too little | 0 after the port |
| `declarationEmit/exportDefaultExpressionComments.ts` | not supported | `export default` is not supported, on ` export default null ` |
| `declarationEmit/exportDefaultNamespace.ts` | not supported | `export default` is not supported, on ` export default function someFunc(): string { ` |
| `declarationEmit/leaveOptionalParameterAsWritten.ts` | multi-file or JavaScript |  |
| `declarationEmit/libReferenceDeclarationEmit.ts` | multi-file or JavaScript |  |
| `declarationEmit/libReferenceDeclarationEmitBundle.ts` | multi-file or JavaScript |  |
| `declarationEmit/libReferenceNoLib.ts` | multi-file or JavaScript |  |
| `declarationEmit/libReferenceNoLibBundle.ts` | multi-file or JavaScript |  |
| `declarationEmit/typeofImportTypeOnlyExport.ts` | multi-file or JavaScript |  |
| `declarationEmit/typePredicates/declarationEmitIdentifierPredicates01.ts` | not supported | `any` is not supported, on ` export function f(x: any): x is number { ` |
| `declarationEmit/typePredicates/declarationEmitIdentifierPredicatesWithPrivateName01.ts` | not supported | `any` is not supported, on ` export function f(x: any): x is I { ` |
| `declarationEmit/typePredicates/declarationEmitThisPredicates01.ts` | not supported | expected type, on ` m(): this is D { ` |
| `declarationEmit/typePredicates/declarationEmitThisPredicates02.ts` | not supported | expected type, on ` m(): this is Foo { ` |
| `declarationEmit/typePredicates/declarationEmitThisPredicatesWithPrivateName01.ts` | not supported | expected type, on ` m(): this is D { ` |
| `declarationEmit/typePredicates/declarationEmitThisPredicatesWithPrivateName02.ts` | not supported | expected type, on ` m(): this is Foo { ` |
| `declarationEmit/typeReferenceRelatedFiles.ts` | multi-file or JavaScript |  |
| `declarationEmit/typesVersionsDeclarationEmit.ambient.ts` | multi-file or JavaScript |  |
| `declarationEmit/typesVersionsDeclarationEmit.multiFile.ts` | multi-file or JavaScript |  |
| `declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToSelf.ts` | multi-file or JavaScript |  |
| `declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToUnmapped.ts` | multi-file or JavaScript |  |
| `decorators/` | not supported | decorators |
| `directives/ts-expect-error-js.ts` | multi-file or JavaScript |  |
| `directives/ts-expect-error-nocheck-js.ts` | multi-file or JavaScript |  |
| `directives/ts-expect-error-nocheck.ts` | checks too little | 1 after the port |
| `directives/ts-expect-error.ts` | porter failure | nothing to prune at offsets 820; our first unsupported error: expected type |
| `dynamicImport/` | not supported | dynamic imports |
| `emitter/es2015/asyncGenerators/emitter.asyncGenerators.classMethods.es2015.ts` | multi-file or JavaScript |  |
| `emitter/es2015/asyncGenerators/emitter.asyncGenerators.functionDeclarations.es2015.ts` | multi-file or JavaScript |  |
| `emitter/es2015/asyncGenerators/emitter.asyncGenerators.functionExpressions.es2015.ts` | multi-file or JavaScript |  |
| `emitter/es2015/asyncGenerators/emitter.asyncGenerators.objectLiteralMethods.es2015.ts` | multi-file or JavaScript |  |
| `emitter/es2018/asyncGenerators/emitter.asyncGenerators.classMethods.es2018.ts` | multi-file or JavaScript |  |
| `emitter/es2018/asyncGenerators/emitter.asyncGenerators.functionDeclarations.es2018.ts` | multi-file or JavaScript |  |
| `emitter/es2018/asyncGenerators/emitter.asyncGenerators.functionExpressions.es2018.ts` | multi-file or JavaScript |  |
| `emitter/es2018/asyncGenerators/emitter.asyncGenerators.objectLiteralMethods.es2018.ts` | multi-file or JavaScript |  |
| `emitter/es2019/noCatchBinding/emitter.noCatchBinding.es2019.ts` | checks too little | 0 after the port |
| `emitter/es5/asyncGenerators/emitter.asyncGenerators.classMethods.es5.ts` | multi-file or JavaScript |  |
| `emitter/es5/asyncGenerators/emitter.asyncGenerators.functionDeclarations.es5.ts` | multi-file or JavaScript |  |
| `emitter/es5/asyncGenerators/emitter.asyncGenerators.functionExpressions.es5.ts` | multi-file or JavaScript |  |
| `emitter/es5/asyncGenerators/emitter.asyncGenerators.objectLiteralMethods.es5.ts` | multi-file or JavaScript |  |
| `enums/awaitAndYield.ts` | the port changes what it checks | `tsc` then reports TS18033, TS2322 |
| `enums/enumBasics.ts` | the port changes what it checks | `tsc` then reports TS7015 |
| `enums/enumClassification.ts` | not supported | mixed numeric and string enum members are not allowed, on ` D = 10, ` |
| `enums/enumConstantMembers.ts` | not supported | expected `;` after expression, on ` declare enum E4 { ` |
| `enums/enumConstantMemberWithString.ts` | not supported | expected `,` or `}` after enum member, on ` b = "1" + "2", ` |
| `enums/enumConstantMemberWithStringEmitDeclaration.ts` | not supported | expected `,` or `}` after enum member, on ` b = "1" + "2", ` |
| `enums/enumConstantMemberWithTemplateLiterals.ts` | not supported | expected a number or string literal for enum initializer, on `` a = `1` `` |
| `enums/enumConstantMemberWithTemplateLiteralsEmitDeclaration.ts` | not supported | expected a number or string literal for enum initializer, on `` a = `1` `` |
| `enums/enumErrorOnConstantBindingWithInitializer.ts` | not supported | default values inside destructuring patterns are not supported, on ` const { value = "123" } = thing; ` |
| `enums/enumErrors.ts` | not supported | expected a number or string literal for enum initializer, on ` C = new Number(30) ` |
| `enums/enumExportMergingES6.ts` | not supported | expected a number or string literal for enum initializer, on ` CatDog = Cat \| Dog ` |
| `enums/enumMerging.ts` | not supported | expected `;` after expression, on ` namespace M1 { ` |
| `enums/enumMergingErrors.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `enums/enumShadowedInfinityNaN.ts` | not supported | expected a number or string literal for enum initializer, on ` X = Infinity ` |
| `es2017/assignSharedArrayBufferToArrayBuffer.ts` | checks too little | 1 after the port |
| `es2017/es2017DateAPIs.ts` | not supported | `Date` is not supported, on ` Date.UTC(2017); ` |
| `es2017/useObjectValuesAndEntries3.ts` | duplicate | of `es2017/useObjectValuesAndEntries2.ts` |
| `es2017/useObjectValuesAndEntries4.ts` | duplicate | of `es2017/useObjectValuesAndEntries2.ts` |
| `es2017/useSharedArrayBuffer2.ts` | duplicate | of `es2017/useSharedArrayBuffer1.ts` |
| `es2017/useSharedArrayBuffer3.ts` | duplicate | of `es2017/useSharedArrayBuffer1.ts` |
| `es2017/useSharedArrayBuffer5.ts` | checks too little | 0 after the port |
| `es2017/useSharedArrayBuffer6.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `es2018/es2018IntlAPIs.ts` | not supported | expected type, on ` const options = { localeMatcher: 'lookup' } as const; ` |
| `es2018/invalidTaggedTemplateEscapeSequences.ts` | not supported | invalid unicode escape: expected hex digits, on `` const x = tag`\u{hello} ${ 100 } \xtraordinary ${ 200 } wonderful ${ 300 } \u... `` |
| `es2018/usePromiseFinally.ts` | checks too little | 1 after the port |
| `es2019/globalThisAmbientModules.ts` | the port changes what it checks | `tsc` then reports TS1039 |
| `es2019/globalThisCollision.ts` | multi-file or JavaScript |  |
| `es2019/globalThisGlobalExportAsGlobal.ts` | not supported | expected `;` after expression, on ` declare global { ` |
| `es2019/globalThisPropertyAssignment.ts` | multi-file or JavaScript |  |
| `es2019/globalThisReadonlyProperties.ts` | not supported | `any` is not supported, on ` globalThis.globalThis = 1 as any // should error ` |
| `es2019/globalThisTypeIndexAccess.ts` | not supported | expected `]` to close array type, on ` const w_e: (typeof globalThis)["globalThis"] = null as unknown as ((typeof gl... ` |
| `es2019/globalThisUnknown.ts` | the port changes what it checks | `tsc` then reports TS7017, TS7015, TS7053 |
| `es2019/globalThisUnknownNoImplicitAny.ts` | not supported | unexpected character `&`, on ` let win: Window & typeof globalThis = null as unknown as (Window & typeof glo... ` |
| `es2019/globalThisVarDeclaration.ts` | multi-file or JavaScript |  |
| `es2019/importMeta/importMeta.ts` | multi-file or JavaScript |  |
| `es2019/importMeta/importMetaNarrowing.ts` | not supported | expected `;` after expression, on ` declare global { interface ImportMeta {foo?: () => void} }; ` |
| `es2020/bigintMissingES2019.ts` | the port changes what it checks | `tsc` then reports TS2559 |
| `es2020/bigintMissingES2020.ts` | the port changes what it checks | `tsc` then reports TS2559 |
| `es2020/bigintMissingESNext.ts` | the port changes what it checks | `tsc` then reports TS2559 |
| `es2020/es2020IntlAPIs.ts` | the port changes what it checks | it leaves an `undefined` it can't rewrite |
| `es2020/intlNumberFormatES2020.ts` | not supported | unknown type `Intl.NumberFormatPartTypes`, on ` const types: Intl.NumberFormatPartTypes[] = [ 'compact', 'unit', 'unknown' ]; ` |
| `es2020/modules/exportAsNamespace_exportAssignment.ts` | multi-file or JavaScript |  |
| `es2020/modules/exportAsNamespace_missingEmitHelpers.ts` | multi-file or JavaScript |  |
| `es2020/modules/exportAsNamespace_nonExistent.ts` | the port changes what it checks | `tsc` then reports TS2792 |
| `es2020/modules/exportAsNamespace1.ts` | multi-file or JavaScript |  |
| `es2020/modules/exportAsNamespace2.ts` | multi-file or JavaScript |  |
| `es2020/modules/exportAsNamespace3.ts` | multi-file or JavaScript |  |
| `es2020/modules/exportAsNamespace4.ts` | multi-file or JavaScript |  |
| `es2020/modules/exportAsNamespace5.ts` | multi-file or JavaScript |  |
| `es2021/es2021LocalesObjectArgument.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `es2021/intlDateTimeFormatRangeES2021.ts` | the port changes what it checks | `tsc` then reports TS2339, TS2551 |
| `es2021/logicalAssignment/logicalAssignment1.ts` | not supported | expected expression, on ` a &&= "foo" ` |
| `es2021/logicalAssignment/logicalAssignment10.ts` | the port changes what it checks | `tsc` then reports TS7053 |
| `es2021/logicalAssignment/logicalAssignment3.ts` | not supported | expected expression, on ` (a.baz) &&= result.baz; ` |
| `es2021/logicalAssignment/logicalAssignment4.ts` | the port changes what it checks | `tsc` then reports TS2322, TS18047 |
| `es2021/logicalAssignment/logicalAssignment5.ts` | not supported | optional function parameters are not yet supported, on ` function foo1 (f?: (a: number) => void): void { ` |
| `es2021/logicalAssignment/logicalAssignment6.ts` | the port changes what it checks | `tsc` then reports TS2531 |
| `es2021/logicalAssignment/logicalAssignment7.ts` | the port changes what it checks | `tsc` then reports TS2531 |
| `es2021/logicalAssignment/logicalAssignment8.ts` | the port changes what it checks | `tsc` then reports TS2531 |
| `es2021/logicalAssignment/logicalAssignment9.ts` | not supported | expected expression, on ` x.a ??= true; ` |
| `es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_exportEmpty.ts` | the port changes what it checks | `tsc` then reports TS18057 |
| `es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_importEmpty.ts` | the port changes what it checks | `tsc` then reports TS18057 |
| `es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts` | not supported | expected local name after `as`, on ` export { someValue as "<X>" }; ` |
| `es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_syntax.ts` | multi-file or JavaScript |  |
| `es2022/es2022IntlAPIs.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `es2022/es2022LocalesObjectArgument.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `es2022/es2024SharedMemory.ts` | the port changes what it checks | `tsc` then reports TS2300 |
| `es2023/intlNumberFormatES2023.ts` | the port changes what it checks | `tsc` then reports TS2339, TS2769, TS2551, TS7006 |
| `es2023/intlNumberFormatES5UseGrouping.ts` | the port changes what it checks | `tsc` then reports TS2769 |
| `es2024/resizableArrayBuffer.ts` | the port changes what it checks | `tsc` then reports TS2554, TS2550 |
| `es2024/sharedMemory.ts` | the port changes what it checks | `tsc` then reports TS2550, TS2300 |
| `es2024/transferableArrayBuffer.ts` | the port changes what it checks | `tsc` then reports TS2550 |
| `es2025/float16Array.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `es2025/intlDurationFormat.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `es2025/regExpEscape.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `es2025/syncIteratorHelpers.ts` | the port changes what it checks | `tsc` then reports TS2339, TS7006 |
| `es5/es5DateAPIs.ts` | not supported | `Date` is not supported, on ` Date.UTC(2017); // should error ` |
| `es6/arrowFunction/disallowLineTerminatorBeforeArrow.ts` | the port changes what it checks | `tsc` then reports TS7019, TS7006 |
| `es6/arrowFunction/emitArrowFunction.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/arrowFunction/emitArrowFunctionAsIs.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionAsIsES6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionES6.ts` | the port changes what it checks | `tsc` then reports TS7019, TS7031 |
| `es6/arrowFunction/emitArrowFunctionsAsIs.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionsAsIsES6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionThisCapturing.ts` | the port changes what it checks | `tsc` then reports TS7041, TS7017 |
| `es6/arrowFunction/emitArrowFunctionThisCapturingES6.ts` | the port changes what it checks | `tsc` then reports TS7041, TS7017 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments01_ES6.ts` | duplicate | of `es6/arrowFunction/emitArrowFunctionWhenUsingArguments01.ts` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments02_ES6.ts` | duplicate | of `es6/arrowFunction/emitArrowFunctionWhenUsingArguments02.ts` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments02.ts` | checks too little | 1 after the port |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments03_ES6.ts` | the port changes what it checks | `tsc` then reports TS7034, TS7005 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments03.ts` | the port changes what it checks | `tsc` then reports TS7034, TS7005 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments04_ES6.ts` | not supported | `let` declaration requires an initializer, on ` let arguments; ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments04.ts` | not supported | `let` declaration requires an initializer, on ` let arguments; ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments05_ES6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments05.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments06_ES6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments06.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments07_ES6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments07.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments08_ES6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments08.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments09_ES6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments09.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments10_ES6.ts` | duplicate | of `es6/arrowFunction/emitArrowFunctionWhenUsingArguments10.ts` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments10.ts` | checks too little | 1 after the port |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments11_ES6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments11.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments12_ES6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments12.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments13_ES6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments13.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments14_ES6.ts` | the port changes what it checks | `tsc` then reports TS2366 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments14.ts` | the port changes what it checks | `tsc` then reports TS2366 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments15_ES6.ts` | the port changes what it checks | `tsc` then reports TS2366 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments15.ts` | the port changes what it checks | `tsc` then reports TS2366 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments16_ES6.ts` | the port changes what it checks | `tsc` then reports TS2366 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments16.ts` | the port changes what it checks | `tsc` then reports TS2366 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments17_ES6.ts` | the port changes what it checks | `tsc` then reports TS2366, TS2451 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments17.ts` | the port changes what it checks | `tsc` then reports TS2366, TS2451 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments18_ES6.ts` | the port changes what it checks | `tsc` then reports TS2366 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments18.ts` | the port changes what it checks | `tsc` then reports TS2366 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments19_ES6.ts` | duplicate | of `es6/arrowFunction/emitArrowFunctionWhenUsingArguments19.ts` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments19.ts` | not supported | `any` is not supported, on ` function foo(x: any): number { ` |
| `es6/binaryAndOctalIntegerLiteral/binaryIntegerLiteral.ts` | not supported | expected field name, on ` 0b11010: "Hello", ` |
| `es6/binaryAndOctalIntegerLiteral/binaryIntegerLiteralError.ts` | not supported | expected `;` after declaration, on ` let bin1 = 0B1102110; ` |
| `es6/binaryAndOctalIntegerLiteral/binaryIntegerLiteralES6.ts` | not supported | expected field name, on ` 0b11010: "Hello", ` |
| `es6/binaryAndOctalIntegerLiteral/invalidBinaryIntegerLiteralAndOctalIntegerLiteral.ts` | not supported | missing digits after `0b`, on ` let binary = 0b21010; ` |
| `es6/binaryAndOctalIntegerLiteral/octalIntegerLiteral.ts` | not supported | expected field name, on ` 0o45436: "Hello", ` |
| `es6/binaryAndOctalIntegerLiteral/octalIntegerLiteralError.ts` | not supported | expected `;` after declaration, on ` let oct1 = 0O13334823; ` |
| `es6/binaryAndOctalIntegerLiteral/octalIntegerLiteralES6.ts` | not supported | expected field name, on ` 0o45436: "Hello", ` |
| `es6/classDeclaration/classWithSemicolonClassElementES61.ts` | duplicate | of `classes/classDeclarations/classWithSemicolonClassElement1.ts` |
| `es6/classDeclaration/classWithSemicolonClassElementES62.ts` | duplicate | of `classes/classDeclarations/classWithSemicolonClassElement1.ts` |
| `es6/classDeclaration/emitClassDeclarationOverloadInES6.ts` | not supported | `any` is not supported, on ` constructor(y: any) ` |
| `es6/classDeclaration/emitClassDeclarationWithConstructorInES6.ts` | the port changes what it checks | `tsc` then reports TS7010, TS7019 |
| `es6/classDeclaration/emitClassDeclarationWithExtensionAndTypeArgumentInES6.ts` | not supported | `any` is not supported, on ` constructor(a: any) ` |
| `es6/classDeclaration/emitClassDeclarationWithGetterSetterInES6.ts` | not supported | static accessors are not supported, on ` static get name2(): string { ` |
| `es6/classDeclaration/emitClassDeclarationWithLiteralPropertyNameInES6.ts` | not supported | class fields require a type annotation, on ` "hello" = 10; ` |
| `es6/classDeclaration/emitClassDeclarationWithMethodInES6.ts` | not supported | expected class member name, on ` ["computedName1"](): void { } ` |
| `es6/classDeclaration/emitClassDeclarationWithPropertyAccessInHeritageClause1.ts` | not supported | expected `)` to close a parenthesized type, on ` class C extends (foo()).B {} ` |
| `es6/classDeclaration/emitClassDeclarationWithPropertyAssignmentInES6.ts` | checks too little | 3 after the port |
| `es6/classDeclaration/emitClassDeclarationWithStaticPropertyAssignmentInES6.ts` | checks too little | 2 after the port |
| `es6/classDeclaration/emitClassDeclarationWithSuperMethodCall01.ts` | checks too little | 3 after the port |
| `es6/classDeclaration/emitClassDeclarationWithTypeArgumentAndOverloadInES6.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `es6/classDeclaration/exportDefaultClassWithStaticPropertyAssignmentsInES6.ts` | not supported | `export default` is not supported, on ` export default class { ` |
| `es6/classDeclaration/parseClassDeclarationInStrictModeByDefaultInES6.ts` | not supported | `any` is not supported, on ` public foo(arguments: any): void { } ` |
| `es6/classDeclaration/superCallBeforeThisAccessing1.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7008, TS2448 |
| `es6/classDeclaration/superCallBeforeThisAccessing2.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7008 |
| `es6/classDeclaration/superCallBeforeThisAccessing3.ts` | not supported | parameter requires a type annotation, on ` constructor(c) { } ` |
| `es6/classDeclaration/superCallBeforeThisAccessing4.ts` | not supported | expected `:` and a type for the class field, on ` private _t; ` |
| `es6/classDeclaration/superCallBeforeThisAccessing5.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `es6/classDeclaration/superCallBeforeThisAccessing6.ts` | not supported | parameter requires a type annotation, on ` constructor(c) { } ` |
| `es6/classDeclaration/superCallBeforeThisAccessing7.ts` | not supported | parameter requires a type annotation, on ` constructor(c) { } ` |
| `es6/classDeclaration/superCallBeforeThisAccessing8.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7008 |
| `es6/classDeclaration/superCallFromClassThatHasNoBaseTypeButWithSameSymbolInterface.ts` | checks too little | 1 after the port |
| `es6/classExpressions/classExpressionES61.ts` | not supported | expected expression, on ` let v = class C {}; ` |
| `es6/classExpressions/classExpressionES62.ts` | not supported | expected expression, on ` let v = class C extends D {}; ` |
| `es6/classExpressions/classExpressionES63.ts` | not supported | expected expression, on ` let C = class extends class extends class { a: number = 1 } { b: number = 2 }... ` |
| `es6/classExpressions/typeArgumentInferenceWithClassExpression1.ts` | not supported | parameter requires a type annotation, on ` function foo<T>(x = class { static prop: T }): T { ` |
| `es6/classExpressions/typeArgumentInferenceWithClassExpression2.ts` | not supported | parameter requires a type annotation, on ` function foo<T>(x = class { prop: T }): T { ` |
| `es6/classExpressions/typeArgumentInferenceWithClassExpression3.ts` | not supported | parameter requires a type annotation, on ` function foo<T>(x = class { prop: T }): T { ` |
| `es6/computedProperties/` | not supported | computed property names |
| `es6/decorators/` | not supported | decorators |
| `es6/defaultParameters/emitDefaultParametersFunction.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/defaultParameters/emitDefaultParametersFunctionES6.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/defaultParameters/emitDefaultParametersFunctionExpression.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/defaultParameters/emitDefaultParametersFunctionExpressionES6.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/defaultParameters/emitDefaultParametersFunctionProperty.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/defaultParameters/emitDefaultParametersFunctionPropertyES6.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/defaultParameters/emitDefaultParametersMethod.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/defaultParameters/emitDefaultParametersMethodES6.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/destructuring/arrayAssignmentPatternWithAny.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/declarationInAmbientContext.ts` | the port changes what it checks | `tsc` then reports TS1182, TS7031 |
| `es6/destructuring/declarationsAndAssignments.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `es6/destructuring/declarationWithNoInitializer.ts` | the port changes what it checks | `tsc` then reports TS7031 |
| `es6/destructuring/destructuringArrayBindingPatternAndAssignment1ES5iterable.ts` | duplicate | of `es6/destructuring/destructuringArrayBindingPatternAndAssignment1ES5.ts` |
| `es6/destructuring/destructuringArrayBindingPatternAndAssignment1ES6.ts` | duplicate | of `es6/destructuring/destructuringArrayBindingPatternAndAssignment1ES5.ts` |
| `es6/destructuring/destructuringArrayBindingPatternAndAssignment2.ts` | not supported | nested destructuring is not supported, on ` let [[a0], [[a1]]] = []         // Error ` |
| `es6/destructuring/destructuringArrayBindingPatternAndAssignment3.ts` | the port changes what it checks | `tsc` then reports TS7022 |
| `es6/destructuring/destructuringArrayBindingPatternAndAssignment5SiblingInitializer.ts` | not supported | default values inside destructuring patterns are not supported, on ` const [a1, b1 = a1] = [1]; ` |
| `es6/destructuring/destructuringAssignabilityCheck.ts` | the port changes what it checks | `tsc` then reports TS2531 |
| `es6/destructuring/destructuringCatch.ts` | the port changes what it checks | `tsc` then reports TS2488, TS2339 |
| `es6/destructuring/destructuringEvaluationOrder.ts` | not supported | `any` is not supported, on ` let trace: any[] = []; ` |
| `es6/destructuring/destructuringInFunctionType.ts` | the port changes what it checks | `tsc` then reports TS7008, TS7031 |
| `es6/destructuring/destructuringObjectAssignmentPatternWithNestedSpread.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any), b: any = null as unknown as (any), c: ... ` |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment1ES5.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448, TS2451 |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment1ES6.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448, TS2451 |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment3.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment4.ts` | not supported | default values inside destructuring patterns are not supported, on ` a = 1, ` |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment5.ts` | not supported | `any` is not supported, on ` let y: any = null as unknown as (any); ` |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment6.ts` | not supported | expected field name in object pattern, on ` const { [a]: aVal, [b]: bVal } = (() => { ` |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment7.ts` | not supported | expected field name in object pattern, on ` const { [K.a]: aVal, [K.b]: bVal } = (() => { ` |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment8.ts` | not supported | expected field name in object pattern, on ` const { [K.a]: aVal, [K.b]: bVal } = (() => { ` |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment9SiblingInitializer.ts` | not supported | default values inside destructuring patterns are not supported, on ` const { a1, b1 = a1 } = { a1: 1 }; ` |
| `es6/destructuring/destructuringParameterDeclaration10.ts` | not supported | nested destructuring is not supported, on ` additionalFiles: { ` |
| `es6/destructuring/destructuringParameterDeclaration1ES5.ts` | not supported | nested destructuring is not supported, on ` function a1([a, b, [[c]]]: [number, number, string[][]]): void { } ` |
| `es6/destructuring/destructuringParameterDeclaration1ES5iterable.ts` | not supported | nested destructuring is not supported, on ` function a1([a, b, [[c]]]: [number, number, string[][]]): void { } ` |
| `es6/destructuring/destructuringParameterDeclaration1ES6.ts` | not supported | nested destructuring is not supported, on ` function a1([a, b, [[c]]]: [number, number, string[][]]): void { } ` |
| `es6/destructuring/destructuringParameterDeclaration2.ts` | not supported | nested destructuring is not supported, on ` function a0([a, b, [[c]]]: [number, number, string[][]]): void { } ` |
| `es6/destructuring/destructuringParameterDeclaration3ES5iterable.ts` | duplicate | of `es6/destructuring/destructuringParameterDeclaration3ES5.ts` |
| `es6/destructuring/destructuringParameterDeclaration3ES6.ts` | duplicate | of `es6/destructuring/destructuringParameterDeclaration3ES5.ts` |
| `es6/destructuring/destructuringParameterDeclaration5.ts` | not supported | expected `,` or `>`, on ` function d0<T extends Class>({x} = { x: new Class() }): void { } ` |
| `es6/destructuring/destructuringParameterDeclaration6.ts` | the port changes what it checks | `tsc` then reports TS1003 |
| `es6/destructuring/destructuringParameterDeclaration7ES5.ts` | not supported | empty object destructuring pattern, on ` function foo({}, {foo, bar}: ISomething): void {} ` |
| `es6/destructuring/destructuringParameterDeclaration7ES5iterable.ts` | not supported | empty object destructuring pattern, on ` function foo({}, {foo, bar}: ISomething): void {} ` |
| `es6/destructuring/destructuringParameterDeclaration8.ts` | not supported | default values inside destructuring patterns are not supported, on ` method = 'z', ` |
| `es6/destructuring/destructuringParameterDeclaration9.ts` | multi-file or JavaScript |  |
| `es6/destructuring/destructuringParameterProperties1.ts` | not supported | parameter requires a type annotation, on ` constructor(public [x, y, z]: string[]) { ` |
| `es6/destructuring/destructuringParameterProperties2.ts` | not supported | parameter requires a type annotation, on ` constructor(private k: number, private [a, b, c]: [number, string, boolean]) { ` |
| `es6/destructuring/destructuringParameterProperties3.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `es6/destructuring/destructuringParameterProperties4.ts` | not supported | parameter requires a type annotation, on ` constructor(private k: T, protected [a, b, c]: [T,U,V]) { ` |
| `es6/destructuring/destructuringParameterProperties5.ts` | not supported | parameter requires a type annotation, on ` constructor(public [{ x1, x2, x3 }, y, z]: TupleType1) { ` |
| `es6/destructuring/destructuringReassignsRightHandSide.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `es6/destructuring/destructuringTypeAssertionsES5_1.ts` | not supported | `any` is not supported, on ` let { x } = <any>foo(); ` |
| `es6/destructuring/destructuringTypeAssertionsES5_2.ts` | not supported | `any` is not supported, on ` let { x } = (<any>foo()); ` |
| `es6/destructuring/destructuringTypeAssertionsES5_3.ts` | not supported | `any` is not supported, on ` let { x } = <any>(foo()); ` |
| `es6/destructuring/destructuringTypeAssertionsES5_4.ts` | not supported | `any` is not supported, on ` let { x } = <any><any>foo(); ` |
| `es6/destructuring/destructuringTypeAssertionsES5_5.ts` | not supported | `any` is not supported, on ` let { x } = <any>0; ` |
| `es6/destructuring/destructuringTypeAssertionsES5_6.ts` | not supported | `any` is not supported, on ` let { x } = <any>new Foo; ` |
| `es6/destructuring/destructuringTypeAssertionsES5_7.ts` | not supported | `any` is not supported, on ` let { x } = <any><any>new Foo; ` |
| `es6/destructuring/destructuringVariableDeclaration1ES5iterable.ts` | duplicate | of `es6/destructuring/destructuringVariableDeclaration1ES5.ts` |
| `es6/destructuring/destructuringVariableDeclaration1ES6.ts` | duplicate | of `es6/destructuring/destructuringVariableDeclaration1ES5.ts` |
| `es6/destructuring/destructuringVariableDeclaration2.ts` | not supported | nested destructuring is not supported, on ` let [a3, [[a4]], a5]: [number, [[string]], boolean] = [1, [[false]], true];  ... ` |
| `es6/destructuring/destructuringVoid.ts` | not supported | empty object destructuring pattern, on ` const {} = v; ` |
| `es6/destructuring/destructuringVoidStrictNullChecks.ts` | not supported | empty object destructuring pattern, on ` const {} = v; ` |
| `es6/destructuring/destructuringWithLiteralInitializers.ts` | the port changes what it checks | `tsc` then reports TS7031 |
| `es6/destructuring/destructuringWithLiteralInitializers2.ts` | not supported | parameter requires a type annotation, on ` function f00([x, y]): void {} ` |
| `es6/destructuring/emptyArrayBindingPatternParameter01.ts` | not supported | empty array destructuring pattern, on ` function f([]): void { ` |
| `es6/destructuring/emptyArrayBindingPatternParameter02.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/destructuring/emptyArrayBindingPatternParameter03.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/destructuring/emptyArrayBindingPatternParameter04.ts` | not supported | empty array destructuring pattern, on ` function f([] = [1,2,3,4]): void { ` |
| `es6/destructuring/emptyAssignmentPatterns01_ES5.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyAssignmentPatterns01_ES5iterable.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyAssignmentPatterns01_ES6.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyAssignmentPatterns02_ES5.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyAssignmentPatterns02_ES5iterable.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyAssignmentPatterns02_ES6.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyAssignmentPatterns03_ES5.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyAssignmentPatterns03_ES5iterable.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyAssignmentPatterns03_ES6.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyAssignmentPatterns04_ES5.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyAssignmentPatterns04_ES5iterable.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyAssignmentPatterns04_ES6.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyObjectBindingPatternParameter01.ts` | not supported | empty object destructuring pattern, on ` function f({}): void { ` |
| `es6/destructuring/emptyObjectBindingPatternParameter02.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/destructuring/emptyObjectBindingPatternParameter03.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/destructuring/emptyObjectBindingPatternParameter04.ts` | not supported | empty object destructuring pattern, on ` function f({} = {a: 1, b: "2", c: true}): void { ` |
| `es6/destructuring/emptyVariableDeclarationBindingPatterns01_ES5.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyVariableDeclarationBindingPatterns01_ES5iterable.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyVariableDeclarationBindingPatterns01_ES6.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyVariableDeclarationBindingPatterns02_ES5.ts` | not supported | empty object destructuring pattern, on ` let {}; ` |
| `es6/destructuring/emptyVariableDeclarationBindingPatterns02_ES5iterable.ts` | not supported | empty object destructuring pattern, on ` let {}; ` |
| `es6/destructuring/emptyVariableDeclarationBindingPatterns02_ES6.ts` | not supported | empty object destructuring pattern, on ` let {}; ` |
| `es6/destructuring/iterableArrayPattern1.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/destructuring/iterableArrayPattern10.ts` | the port changes what it checks | `tsc` then reports TS7008, TS7031 |
| `es6/destructuring/iterableArrayPattern11.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `es6/destructuring/iterableArrayPattern12.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `es6/destructuring/iterableArrayPattern13.ts` | the port changes what it checks | `tsc` then reports TS7008, TS7031 |
| `es6/destructuring/iterableArrayPattern14.ts` | the port changes what it checks | `tsc` then reports TS7008, TS7031 |
| `es6/destructuring/iterableArrayPattern15.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `es6/destructuring/iterableArrayPattern16.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `es6/destructuring/iterableArrayPattern17.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `es6/destructuring/iterableArrayPattern18.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `es6/destructuring/iterableArrayPattern19.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `es6/destructuring/iterableArrayPattern2.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/destructuring/iterableArrayPattern20.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `es6/destructuring/iterableArrayPattern21.ts` | not supported | expected field name, on ` let [a, b] = { 0: "", 1: true }; ` |
| `es6/destructuring/iterableArrayPattern22.ts` | not supported | expected field name, on ` let [...a] = { 0: "", 1: true }; ` |
| `es6/destructuring/iterableArrayPattern23.ts` | not supported | expected `;` after declaration, on ` let a: string = null as unknown as (string), b: boolean = null as unknown as ... ` |
| `es6/destructuring/iterableArrayPattern24.ts` | not supported | expected `;` after declaration, on ` let a: string = null as unknown as (string), b: boolean[] = null as unknown a... ` |
| `es6/destructuring/iterableArrayPattern25.ts` | the port changes what it checks | `tsc` then reports TS7031 |
| `es6/destructuring/iterableArrayPattern26.ts` | not supported | rest parameter cannot be destructured, on ` function takeFirstTwoEntries(...[[k1, v1], [k2, v2]]: [string, number][]): vo... ` |
| `es6/destructuring/iterableArrayPattern27.ts` | not supported | rest parameter cannot be destructured, on ` function takeFirstTwoEntries(...[[k1, v1], [k2, v2]]: [string, number][]): vo... ` |
| `es6/destructuring/iterableArrayPattern28.ts` | not supported | rest parameter cannot be destructured, on ` function takeFirstTwoEntries(...[[k1, v1], [k2, v2]]: [string, number][]): vo... ` |
| `es6/destructuring/iterableArrayPattern29.ts` | not supported | rest parameter cannot be destructured, on ` function takeFirstTwoEntries(...[[k1, v1], [k2, v2]]: [string, number][]): vo... ` |
| `es6/destructuring/iterableArrayPattern3.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `es6/destructuring/iterableArrayPattern30.ts` | not supported | nested destructuring is not supported, on ` const [[k1, v1], [k2, v2]] = new Map([["", true], ["hello", true]]) ` |
| `es6/destructuring/iterableArrayPattern4.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `es6/destructuring/iterableArrayPattern5.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `es6/destructuring/iterableArrayPattern6.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `es6/destructuring/iterableArrayPattern7.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `es6/destructuring/iterableArrayPattern8.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `es6/destructuring/iterableArrayPattern9.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `es6/destructuring/missingAndExcessProperties.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `es6/destructuring/nonIterableRestElement1.ts` | not supported | invalid assignment target, on ` [...c] = ["", 0]; ` |
| `es6/destructuring/nonIterableRestElement2.ts` | not supported | invalid assignment target, on ` [...c] = ["", 0]; ` |
| `es6/destructuring/nonIterableRestElement3.ts` | not supported | invalid assignment target, on ` [...c] = ["", 0]; ` |
| `es6/destructuring/objectBindingPatternKeywordIdentifiers01.ts` | not supported | expected `:` after keyword field name in object pattern, on ` let { while } = { while: 1 } ` |
| `es6/destructuring/objectBindingPatternKeywordIdentifiers02.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `es6/destructuring/objectBindingPatternKeywordIdentifiers03.ts` | not supported | expected field name in object pattern, on ` let { "while" } = { while: 1 } ` |
| `es6/destructuring/objectBindingPatternKeywordIdentifiers04.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `es6/destructuring/objectBindingPatternKeywordIdentifiers05.ts` | checks too little | 1 after the port |
| `es6/destructuring/objectBindingPatternKeywordIdentifiers06.ts` | checks too little | 1 after the port |
| `es6/destructuring/optionalBindingParameters1.ts` | not supported | optional function parameters are not yet supported, on ` function foo([x,y,z]?: [string, number, boolean]): void { ` |
| `es6/destructuring/optionalBindingParameters2.ts` | not supported | optional function parameters are not yet supported, on ` function foo({ x, y, z }?: { x: string; y: number; z: boolean }): void { ` |
| `es6/destructuring/optionalBindingParameters3.ts` | multi-file or JavaScript |  |
| `es6/destructuring/optionalBindingParameters4.ts` | multi-file or JavaScript |  |
| `es6/destructuring/optionalBindingParametersInOverloads1.ts` | not supported | optional function parameters are not yet supported, on ` function foo([x, y, z] ?: [string, number, boolean]); ` |
| `es6/destructuring/optionalBindingParametersInOverloads2.ts` | not supported | optional function parameters are not yet supported, on ` function foo({ x, y, z }?: { x: string; y: number; z: boolean }); ` |
| `es6/destructuring/restElementWithAssignmentPattern1.ts` | not supported | expected `;` after declaration, on ` let a: string = null as unknown as (string), b: number = null as unknown as (... ` |
| `es6/destructuring/restElementWithAssignmentPattern2.ts` | not supported | expected `;` after declaration, on ` let a: string = null as unknown as (string), b: number = null as unknown as (... ` |
| `es6/destructuring/restElementWithAssignmentPattern3.ts` | not supported | expected `;` after declaration, on ` let a: string = null as unknown as (string), b: number = null as unknown as (... ` |
| `es6/destructuring/restElementWithAssignmentPattern4.ts` | not supported | expected `;` after declaration, on ` let a: string = null as unknown as (string), b: number = null as unknown as (... ` |
| `es6/destructuring/restElementWithAssignmentPattern5.ts` | not supported | expected `;` after declaration, on ` let s: string = null as unknown as (string), s2: string = null as unknown as ... ` |
| `es6/destructuring/restElementWithBindingPattern.ts` | not supported | expected identifier after `...`, on ` let [...[a, b]] = [0, 1]; ` |
| `es6/destructuring/restElementWithBindingPattern2.ts` | not supported | expected identifier after `...`, on ` let [...{0: a, b }] = [0, 1]; ` |
| `es6/destructuring/restElementWithInitializer1.ts` | checks too little | 4 after the port |
| `es6/destructuring/restElementWithInitializer2.ts` | not supported | invalid assignment target, on ` [...x = a] = a;  // Error, rest element cannot have initializer ` |
| `es6/destructuring/restElementWithNullInitializer.ts` | not supported | parameter requires a type annotation, on ` function foo1([...r] = null): void { ` |
| `es6/destructuring/restPropertyWithBindingPattern.ts` | not supported | invalid assignment target, on ` ({...{}} = {}); ` |
| `es6/for-ofStatements/for-of-excess-declarations.ts` | not supported | `const` declaration requires an initializer, on ` for (const a, { [b]: c} of [1]) { ` |
| `es6/for-ofStatements/for-of1.ts` | not supported | `let` declaration requires an initializer, on ` let v; ` |
| `es6/for-ofStatements/for-of10.ts` | not supported | expected `;` after expression, on ` for (v of [0]) { } ` |
| `es6/for-ofStatements/for-of11.ts` | not supported | expected `;` after expression, on ` for (v of [0, ""]) { } ` |
| `es6/for-ofStatements/for-of12.ts` | not supported | expected `;` after expression, on ` for (v of [0, ""].values()) { } ` |
| `es6/for-ofStatements/for-of13.ts` | not supported | expected `;` after expression, on ` for (v of [""].values()) { } ` |
| `es6/for-ofStatements/for-of14.ts` | not supported | expected `;` after expression, on ` for (v of new MyStringIterator) { } // Should fail because the iterator is no... ` |
| `es6/for-ofStatements/for-of15.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/for-ofStatements/for-of16.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/for-ofStatements/for-of17.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/for-ofStatements/for-of18.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/for-ofStatements/for-of19.ts` | not supported | expected `(` after constructor name in `new` expression, on ` value: new Foo, ` |
| `es6/for-ofStatements/for-of2.ts` | not supported | `const` declaration requires an initializer, on ` const v; ` |
| `es6/for-ofStatements/for-of20.ts` | not supported | expected `(` after constructor name in `new` expression, on ` value: new Foo, ` |
| `es6/for-ofStatements/for-of21.ts` | not supported | expected `(` after constructor name in `new` expression, on ` value: new Foo, ` |
| `es6/for-ofStatements/for-of22.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `es6/for-ofStatements/for-of23.ts` | not supported | expected `(` after constructor name in `new` expression, on ` value: new Foo, ` |
| `es6/for-ofStatements/for-of24.ts` | not supported | `any` is not supported, on ` let x: any = null as unknown as (any); ` |
| `es6/for-ofStatements/for-of25.ts` | not supported | expected class member name, on ` [Symbol.iterator](): any { ` |
| `es6/for-ofStatements/for-of26.ts` | not supported | `any` is not supported, on ` next(): any { ` |
| `es6/for-ofStatements/for-of27.ts` | not supported | expected class member name, on ` [Symbol.iterator]: any; ` |
| `es6/for-ofStatements/for-of28.ts` | not supported | `any` is not supported, on ` next: any; ` |
| `es6/for-ofStatements/for-of29.ts` | not supported | expected `:` after index parameter name, on ` [Symbol.iterator]?(): Iterator<string> ` |
| `es6/for-ofStatements/for-of3.ts` | not supported | `any` is not supported, on ` let v: any = null as unknown as (any); ` |
| `es6/for-ofStatements/for-of30.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/for-ofStatements/for-of31.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/for-ofStatements/for-of32.ts` | the port changes what it checks | `tsc` then reports TS2448 |
| `es6/for-ofStatements/for-of33.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `es6/for-ofStatements/for-of34.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `es6/for-ofStatements/for-of35.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `es6/for-ofStatements/for-of4.ts` | checks too little | 3 after the port |
| `es6/for-ofStatements/for-of40.ts` | not supported | default values inside destructuring patterns are not supported, on ` for (let [k = "", v = false] of map) { ` |
| `es6/for-ofStatements/for-of41.ts` | not supported | nested destructuring is not supported, on ` for (let {x: [a], y: {p}} of array) { ` |
| `es6/for-ofStatements/for-of42.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (let {x: a, y: b} of array) { ` |
| `es6/for-ofStatements/for-of43.ts` | not supported | default values inside destructuring patterns are not supported, on ` for (let {x: a = "", y: b = true} of array) { ` |
| `es6/for-ofStatements/for-of44.ts` | not supported | unknown type `symbol`, on ` let array: [number, string \| boolean \| symbol][] = [[0, ""], [0, true], [1, S... ` |
| `es6/for-ofStatements/for-of45.ts` | not supported | expected `;` after declaration, on ` let k: string = null as unknown as (string), v: boolean = null as unknown as ... ` |
| `es6/for-ofStatements/for-of46.ts` | not supported | expected `;` after declaration, on ` let k: string = null as unknown as (string), v: boolean = null as unknown as ... ` |
| `es6/for-ofStatements/for-of47.ts` | not supported | expected `;` after declaration, on ` let x: string = null as unknown as (string), y: number = null as unknown as (... ` |
| `es6/for-ofStatements/for-of48.ts` | not supported | expected `;` after declaration, on ` let x: string = null as unknown as (string), y: number = null as unknown as (... ` |
| `es6/for-ofStatements/for-of49.ts` | not supported | expected `;` after declaration, on ` let k: string = null as unknown as (string), v: boolean = null as unknown as ... ` |
| `es6/for-ofStatements/for-of5.ts` | duplicate | of `es6/for-ofStatements/for-of4.ts` |
| `es6/for-ofStatements/for-of51.ts` | not supported | `let` is a reserved keyword and can't be used as a name, on ` for (let let of []) {} ` |
| `es6/for-ofStatements/for-of52.ts` | checks too little | 1 after the port |
| `es6/for-ofStatements/for-of53.ts` | not supported | `let` declaration requires an initializer, on ` let v; ` |
| `es6/for-ofStatements/for-of54.ts` | checks too little | 1 after the port |
| `es6/for-ofStatements/for-of55.ts` | checks too little | 4 after the port |
| `es6/for-ofStatements/for-of56.ts` | the port changes what it checks | `tsc` then reports TS2480 |
| `es6/for-ofStatements/for-of57.ts` | not supported | `as` to `Iterable<number>` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let iter: Iterable<number> = null as unknown as (Iterable<number>); ` |
| `es6/for-ofStatements/for-of58.ts` | not supported | unexpected character `&`, on ` const arr: X[] & Y[] = null as unknown as (X[] & Y[]); ` |
| `es6/for-ofStatements/for-of6.ts` | not supported | expected `;` after expression, on ` for (v of [0]) { ` |
| `es6/for-ofStatements/for-of7.ts` | checks too little | 3 after the port |
| `es6/for-ofStatements/for-of8.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `es6/for-ofStatements/for-of9.ts` | not supported | expected `;` after expression, on ` for (v of ["hello"]) { } ` |
| `es6/functionDeclarations/FunctionDeclaration1_es6.ts` | not supported | expected function name, on ` function * foo(): Generator<never, void, unknown> { ` |
| `es6/functionDeclarations/FunctionDeclaration10_es6.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `es6/functionDeclarations/FunctionDeclaration11_es6.ts` | not supported | expected function name, on ` function * yield(): Generator<never, void, unknown> { ` |
| `es6/functionDeclarations/FunctionDeclaration12_es6.ts` | not supported | expected `(` after `function`, on ` let v = function * yield() { } ` |
| `es6/functionDeclarations/FunctionDeclaration13_es6.ts` | not supported | expected function name, on ` function * foo(): Generator<never, void, unknown> { ` |
| `es6/functionDeclarations/FunctionDeclaration2_es6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/functionDeclarations/FunctionDeclaration3_es6.ts` | not supported | parameter requires a type annotation, on ` function f(yield = yield): void { ` |
| `es6/functionDeclarations/FunctionDeclaration4_es6.ts` | checks too little | 1 after the port |
| `es6/functionDeclarations/FunctionDeclaration5_es6.ts` | not supported | expected function name, on ` function*foo(yield): Generator<never, void, unknown> { ` |
| `es6/functionDeclarations/FunctionDeclaration6_es6.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `es6/functionDeclarations/FunctionDeclaration7_es6.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `es6/functionDeclarations/FunctionDeclaration8_es6.ts` | checks too little | 1 after the port |
| `es6/functionDeclarations/FunctionDeclaration9_es6.ts` | the port changes what it checks | `tsc` then reports TS2464, TS2322 |
| `es6/functionExpressions/FunctionExpression1_es6.ts` | not supported | expected `(` after `function`, on ` let v = function * () { } ` |
| `es6/functionExpressions/FunctionExpression2_es6.ts` | not supported | expected `(` after `function`, on ` let v = function * foo() { } ` |
| `es6/functionPropertyAssignments/FunctionPropertyAssignments1_es6.ts` | not supported | expected field name, on ` let v = { *foo() { } } ` |
| `es6/functionPropertyAssignments/FunctionPropertyAssignments2_es6.ts` | not supported | expected field name, on ` let v = { *() { } } ` |
| `es6/functionPropertyAssignments/FunctionPropertyAssignments3_es6.ts` | not supported | expected field name, on ` let v = { *{ } } ` |
| `es6/functionPropertyAssignments/FunctionPropertyAssignments4_es6.ts` | not supported | expected field name, on ` let v = { * } ` |
| `es6/functionPropertyAssignments/FunctionPropertyAssignments5_es6.ts` | not supported | expected field name, on ` let v = { *[foo()]() { } } ` |
| `es6/functionPropertyAssignments/FunctionPropertyAssignments6_es6.ts` | not supported | expected field name, on ` let v = { *<T>() { } } ` |
| `es6/memberFunctionDeclarations/MemberFunctionDeclaration1_es6.ts` | not supported | expected class member name, on ` *foo(): Generator<never, void, unknown> { } ` |
| `es6/memberFunctionDeclarations/MemberFunctionDeclaration2_es6.ts` | not supported | expected `:` and a type for the class field, on ` public * foo(): Generator<never, void, unknown> { } ` |
| `es6/memberFunctionDeclarations/MemberFunctionDeclaration3_es6.ts` | not supported | expected class member name, on ` *[foo](): Generator<never, void, unknown> { } ` |
| `es6/memberFunctionDeclarations/MemberFunctionDeclaration4_es6.ts` | not supported | expected class member name, on ` *(): Generator<never, void, unknown> { } ` |
| `es6/memberFunctionDeclarations/MemberFunctionDeclaration5_es6.ts` | not supported | expected class member name, on ` * ` |
| `es6/memberFunctionDeclarations/MemberFunctionDeclaration6_es6.ts` | not supported | expected class member name, on ` *foo ` |
| `es6/memberFunctionDeclarations/MemberFunctionDeclaration7_es6.ts` | not supported | expected class member name, on ` *foo<T>(): Generator<never, void, unknown> { } ` |
| `es6/memberFunctionDeclarations/MemberFunctionDeclaration8_es6.ts` | not supported | unexpected character `¬`, on ` if (a) ¬ * bar; ` |
| `es6/moduleExportsAmd/anonymousDefaultExportsAmd.ts` | multi-file or JavaScript |  |
| `es6/moduleExportsAmd/decoratedDefaultExportsGetExportedAmd.ts` | multi-file or JavaScript |  |
| `es6/moduleExportsAmd/defaultExportsGetExportedAmd.ts` | multi-file or JavaScript |  |
| `es6/moduleExportsAmd/outFilerootDirModuleNamesAmd.ts` | multi-file or JavaScript |  |
| `es6/moduleExportsCommonjs/anonymousDefaultExportsCommonjs.ts` | multi-file or JavaScript |  |
| `es6/moduleExportsCommonjs/decoratedDefaultExportsGetExportedCommonjs.ts` | multi-file or JavaScript |  |
| `es6/moduleExportsCommonjs/defaultExportsGetExportedCommonjs.ts` | multi-file or JavaScript |  |
| `es6/moduleExportsSystem/anonymousDefaultExportsSystem.ts` | multi-file or JavaScript |  |
| `es6/moduleExportsSystem/decoratedDefaultExportsGetExportedSystem.ts` | multi-file or JavaScript |  |
| `es6/moduleExportsSystem/defaultExportsGetExportedSystem.ts` | multi-file or JavaScript |  |
| `es6/moduleExportsSystem/outFilerootDirModuleNamesSystem.ts` | multi-file or JavaScript |  |
| `es6/moduleExportsSystem/topLevelVarHoistingCommonJS.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `es6/moduleExportsSystem/topLevelVarHoistingSystem.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `es6/moduleExportsUmd/anonymousDefaultExportsUmd.ts` | multi-file or JavaScript |  |
| `es6/moduleExportsUmd/decoratedDefaultExportsGetExportedUmd.ts` | multi-file or JavaScript |  |
| `es6/moduleExportsUmd/defaultExportsGetExportedUmd.ts` | multi-file or JavaScript |  |
| `es6/modules/defaultExportInAwaitExpression01.ts` | multi-file or JavaScript |  |
| `es6/modules/defaultExportInAwaitExpression02.ts` | multi-file or JavaScript |  |
| `es6/modules/defaultExportsCannotMerge01.ts` | multi-file or JavaScript |  |
| `es6/modules/defaultExportsCannotMerge02.ts` | multi-file or JavaScript |  |
| `es6/modules/defaultExportsCannotMerge03.ts` | multi-file or JavaScript |  |
| `es6/modules/defaultExportsCannotMerge04.ts` | the port changes what it checks | `tsc` then reports TS7005 |
| `es6/modules/defaultExportWithOverloads01.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `es6/modules/exportAndImport-es5-amd.ts` | multi-file or JavaScript |  |
| `es6/modules/exportAndImport-es5.ts` | multi-file or JavaScript |  |
| `es6/modules/exportBinding.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImports1-amd.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImports1-es6.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImports1.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImports2-amd.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImports2-es6.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImports2.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImports3-amd.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImports3-es6.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImports3.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImports4-amd.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImports4-es6.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImports4.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImports5.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImportsWithContextualKeywordNames01.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImportsWithContextualKeywordNames02.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImportsWithUnderscores1.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImportsWithUnderscores2.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImportsWithUnderscores3.ts` | multi-file or JavaScript |  |
| `es6/modules/exportsAndImportsWithUnderscores4.ts` | multi-file or JavaScript |  |
| `es6/modules/exportSpellingSuggestion.ts` | multi-file or JavaScript |  |
| `es6/modules/exportStar-amd.ts` | multi-file or JavaScript |  |
| `es6/modules/exportStar.ts` | multi-file or JavaScript |  |
| `es6/modules/importEmptyFromModuleNotExisted.ts` | the port changes what it checks | `tsc` then reports TS2792 |
| `es6/modules/multipleDefaultExports01.ts` | multi-file or JavaScript |  |
| `es6/modules/multipleDefaultExports02.ts` | multi-file or JavaScript |  |
| `es6/modules/multipleDefaultExports03.ts` | not supported | `export default` is not supported, on ` export default class C { ` |
| `es6/modules/multipleDefaultExports04.ts` | not supported | `export default` is not supported, on ` export default function f(): void { ` |
| `es6/modules/multipleDefaultExports05.ts` | not supported | `export default` is not supported, on ` export default class AA1 {} ` |
| `es6/modules/reExportDefaultExport.ts` | multi-file or JavaScript |  |
| `es6/newTarget/invalidNewTarget.es5.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `es6/newTarget/invalidNewTarget.es6.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `es6/newTarget/newTarget.es5.ts` | not supported | expected expression, on ` const a = new.target; ` |
| `es6/newTarget/newTarget.es6.ts` | not supported | expected expression, on ` const a = new.target; ` |
| `es6/newTarget/newTargetNarrowing.ts` | not supported | expected expression, on ` if (new.target.marked === true) { ` |
| `es6/propertyAccess/propertyAccessNumericLiterals.es6.ts` | not supported | expected field name after `.`, on ` 1234..toString(); ` |
| `es6/restParameters/emitRestParametersFunction.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/restParameters/emitRestParametersFunctionES6.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/restParameters/emitRestParametersFunctionExpression.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/restParameters/emitRestParametersFunctionExpressionES6.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/restParameters/emitRestParametersFunctionProperty.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/restParameters/emitRestParametersFunctionPropertyES6.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/restParameters/emitRestParametersMethod.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/restParameters/emitRestParametersMethodES6.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandProperties.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448 |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesAssignment.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2322 |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesAssignmentError.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesAssignmentErrorFromMissingIdentifier.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesAssignmentES6.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2322 |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesErrorFromNoneExistingIdentifier.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448 |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesErrorFromNotUsingIdentifier.ts` | the port changes what it checks | `tsc` then reports TS7032 |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesErrorWithModule.ts` | the port changes what it checks | `tsc` then reports TS7005 |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesES6.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448 |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesFunctionArgument.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2345 |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesFunctionArgument2.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesWithModule.ts` | the port changes what it checks | `tsc` then reports TS7005 |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesWithModuleES6.ts` | the port changes what it checks | `tsc` then reports TS7005 |
| `es6/spread/arrayLiteralSpreadES5iterable.ts` | duplicate | of `es6/spread/arrayLiteralSpread.ts` |
| `es6/spread/arraySpreadImportHelpers.ts` | multi-file or JavaScript |  |
| `es6/spread/arraySpreadInCall.ts` | not supported | expected expression, on ` f1(1, 2, 3, 4, ...[5, 6]); ` |
| `es6/spread/iteratorSpreadInArray.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInArray10.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInArray11.ts` | not supported | `as` to `Iterable<number>` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let iter: Iterable<number> = null as unknown as (Iterable<number>); ` |
| `es6/spread/iteratorSpreadInArray2.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInArray3.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInArray4.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInArray5.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInArray6.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInArray7.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInArray8.ts` | not supported | expected `(` after constructor name in `new` expression, on ` let array = [...new SymbolIterator]; ` |
| `es6/spread/iteratorSpreadInArray9.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInCall.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInCall10.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInCall11.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInCall12.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInCall2.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInCall3.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInCall4.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInCall5.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInCall6.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInCall7.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInCall8.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/spread/iteratorSpreadInCall9.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/Symbols/` | not supported | `Symbol` |
| `es6/templates/taggedTemplateStringsPlainCharactersThatArePartsOfEscapes01_ES6.ts` | not supported | `any` is not supported, on ` function f(...x: any[]): void { ` |
| `es6/templates/taggedTemplateStringsPlainCharactersThatArePartsOfEscapes01.ts` | not supported | `any` is not supported, on ` function f(...x: any[]): void { ` |
| `es6/templates/taggedTemplateStringsPlainCharactersThatArePartsOfEscapes02_ES6.ts` | not supported | `any` is not supported, on ` function f(...x: any[]): void { ` |
| `es6/templates/taggedTemplateStringsPlainCharactersThatArePartsOfEscapes02.ts` | checks too little | 1 after the port |
| `es6/templates/taggedTemplateStringsTypeArgumentInference.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `es6/templates/taggedTemplateStringsTypeArgumentInferenceES6.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `es6/templates/taggedTemplateStringsWithIncompatibleTypedTags.ts` | not supported | expected `]` to close array type, on ` [x: number]: I; ` |
| `es6/templates/taggedTemplateStringsWithIncompatibleTypedTagsES6.ts` | not supported | expected `]` to close array type, on ` [x: number]: I; ` |
| `es6/templates/taggedTemplateStringsWithManyCallAndMemberExpressions.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `es6/templates/taggedTemplateStringsWithManyCallAndMemberExpressionsES6.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `es6/templates/taggedTemplateStringsWithOverloadResolution1_ES6.ts` | not supported | expected `{`, on ` function foo(strs: TemplateStringsArray): number; ` |
| `es6/templates/taggedTemplateStringsWithOverloadResolution1.ts` | not supported | expected `{`, on ` function foo(strs: TemplateStringsArray): number; ` |
| `es6/templates/taggedTemplateStringsWithOverloadResolution2_ES6.ts` | the port changes what it checks | it leaves an `undefined` it can't rewrite |
| `es6/templates/taggedTemplateStringsWithOverloadResolution2.ts` | the port changes what it checks | it leaves an `undefined` it can't rewrite |
| `es6/templates/taggedTemplateStringsWithOverloadResolution3_ES6.ts` | the port changes what it checks | it leaves an `undefined` it can't rewrite |
| `es6/templates/taggedTemplateStringsWithOverloadResolution3.ts` | the port changes what it checks | it leaves an `undefined` it can't rewrite |
| `es6/templates/taggedTemplateStringsWithTagNamedDeclare.ts` | not supported | `any` is not supported, on ` function declare(x: any, ...ys: any[]): void { ` |
| `es6/templates/taggedTemplateStringsWithTagNamedDeclareES6.ts` | not supported | `any` is not supported, on ` function declare(x: any, ...ys: any[]): void { ` |
| `es6/templates/taggedTemplateStringsWithTagsTypedAsAny.ts` | not supported | `any` is not supported, on ` let f: any = null as unknown as (any); ` |
| `es6/templates/taggedTemplateStringsWithTagsTypedAsAnyES6.ts` | not supported | `any` is not supported, on ` let f: any = null as unknown as (any); ` |
| `es6/templates/taggedTemplateStringsWithTypedTags.ts` | not supported | expected `]` to close array type, on ` [x: number]: I; ` |
| `es6/templates/taggedTemplateStringsWithTypedTagsES6.ts` | not supported | expected `]` to close array type, on ` [x: number]: I; ` |
| `es6/templates/taggedTemplateStringsWithTypeErrorInFunctionExpressionsInSubstitutionExpression.ts` | not supported | `any` is not supported, on ` function foo(...rest: any[]): void { ` |
| `es6/templates/taggedTemplateStringsWithTypeErrorInFunctionExpressionsInSubstitutionExpressionES6.ts` | not supported | `any` is not supported, on ` function foo(...rest: any[]): void { ` |
| `es6/templates/taggedTemplatesWithTypeArguments1.ts` | not supported | `any` is not supported, on ` function f<T>(strs: TemplateStringsArray, ...callbacks: Array<(x: T) => any>)... ` |
| `es6/templates/taggedTemplatesWithTypeArguments2.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `es6/templates/taggedTemplateUntypedTagCall01.ts` | not supported | expected `;` after expression, on `` tag `Hello world!`; `` |
| `es6/templates/taggedTemplateWithConstructableTag01.ts` | not supported | expected `;` after expression, on `` CtorTag `Hello world!`; `` |
| `es6/templates/taggedTemplateWithConstructableTag02.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `es6/templates/TemplateExpression1.ts` | porter failure | nothing to prune at offsets 39; our first unsupported error: expected `}` to close template interpolation |
| `es6/templates/templateStringBinaryOperations.ts` | not supported | unexpected character `&`, on `` var a4 = 1 + `${ 3 & 4 }`; `` |
| `es6/templates/templateStringBinaryOperationsES6.ts` | not supported | unexpected character `&`, on `` var a4 = 1 + `${ 3 & 4 }`; `` |
| `es6/templates/templateStringBinaryOperationsES6Invalid.ts` | not supported | unexpected character `&`, on `` var a3 = 1 & `${ 3 }`; `` |
| `es6/templates/templateStringBinaryOperationsInvalid.ts` | not supported | unexpected character `&`, on `` var a3 = 1 & `${ 3 }`; `` |
| `es6/templates/templateStringControlCharacterEscapes01_ES6.ts` | not supported | unknown escape sequence `\x`, on `` let x = `\0\x00\u0000 0 00 0000`; `` |
| `es6/templates/templateStringControlCharacterEscapes01.ts` | not supported | unknown escape sequence `\x`, on `` let x = `\0\x00\u0000 0 00 0000`; `` |
| `es6/templates/templateStringControlCharacterEscapes02_ES6.ts` | not supported | unknown escape sequence `\x`, on `` let x = `\x19\u0019 19`; `` |
| `es6/templates/templateStringControlCharacterEscapes02.ts` | not supported | unknown escape sequence `\x`, on `` let x = `\x19\u0019 19`; `` |
| `es6/templates/templateStringControlCharacterEscapes03_ES6.ts` | not supported | unknown escape sequence `\x`, on `` let x = `\x1F\u001f 1F 1f`; `` |
| `es6/templates/templateStringControlCharacterEscapes03.ts` | not supported | unknown escape sequence `\x`, on `` let x = `\x1F\u001f 1F 1f`; `` |
| `es6/templates/templateStringControlCharacterEscapes04_ES6.ts` | not supported | unknown escape sequence `\x`, on `` let x = `\x20\u0020 20`; `` |
| `es6/templates/templateStringControlCharacterEscapes04.ts` | not supported | unknown escape sequence `\x`, on `` let x = `\x20\u0020 20`; `` |
| `es6/templates/templateStringInArray.ts` | checks too little | 3 after the port |
| `es6/templates/templateStringInArrowFunction.ts` | checks too little | 1 after the port |
| `es6/templates/templateStringInArrowFunctionES6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/templates/templateStringInCallExpression.ts` | checks too little | 4 after the port |
| `es6/templates/templateStringInCallExpressionES6.ts` | duplicate | of `es6/templates/templateStringInCallExpression.ts` |
| `es6/templates/templateStringInConditionalES6.ts` | duplicate | of `es6/templates/templateStringInConditional.ts` |
| `es6/templates/templateStringInDeleteExpression.ts` | not supported | `delete` |
| `es6/templates/templateStringInDeleteExpressionES6.ts` | not supported | `delete` |
| `es6/templates/templateStringInDivision.ts` | checks too little | 2 after the port |
| `es6/templates/templateStringInEqualityChecksES6.ts` | duplicate | of `es6/templates/templateStringInEqualityChecks.ts` |
| `es6/templates/templateStringInFunctionExpression.ts` | checks too little | 4 after the port |
| `es6/templates/templateStringInFunctionExpressionES6.ts` | duplicate | of `es6/templates/templateStringInFunctionExpression.ts` |
| `es6/templates/templateStringInFunctionParameterType.ts` | not supported | expected parameter name, on `` function f(`hello`); `` |
| `es6/templates/templateStringInFunctionParameterTypeES6.ts` | not supported | expected parameter name, on `` function f(`hello`); `` |
| `es6/templates/templateStringInIndexExpression.ts` | checks too little | 1 after the port |
| `es6/templates/templateStringInIndexExpressionES6.ts` | duplicate | of `es6/templates/templateStringInIndexExpression.ts` |
| `es6/templates/templateStringInInOperator.ts` | not supported | expected `,` or `}`, on `` let x = `${ "hi" }` in { hi: 10, hello: 20}; `` |
| `es6/templates/templateStringInInOperatorES6.ts` | not supported | expected `,` or `}`, on `` let x = `${ "hi" }` in { hi: 10, hello: 20}; `` |
| `es6/templates/templateStringInInstanceOf.ts` | checks too little | 4 after the port |
| `es6/templates/templateStringInInstanceOfES6.ts` | duplicate | of `es6/templates/templateStringInInstanceOf.ts` |
| `es6/templates/templateStringInModuleName.ts` | the port changes what it checks | `tsc` then reports TS2580 |
| `es6/templates/templateStringInModuleNameES6.ts` | the port changes what it checks | `tsc` then reports TS2580 |
| `es6/templates/templateStringInModulo.ts` | checks too little | 2 after the port |
| `es6/templates/templateStringInModuloES6.ts` | duplicate | of `es6/templates/templateStringInModulo.ts` |
| `es6/templates/templateStringInMultiplication.ts` | checks too little | 2 after the port |
| `es6/templates/templateStringInMultiplicationES6.ts` | duplicate | of `es6/templates/templateStringInMultiplication.ts` |
| `es6/templates/templateStringInNewExpression.ts` | checks too little | 2 after the port |
| `es6/templates/templateStringInNewExpressionES6.ts` | duplicate | of `es6/templates/templateStringInNewExpression.ts` |
| `es6/templates/templateStringInNewOperator.ts` | not supported | expected `(` after constructor name in `new` expression, on `` let x = new `abc${ 1 }def`; `` |
| `es6/templates/templateStringInNewOperatorES6.ts` | not supported | expected `(` after constructor name in `new` expression, on `` let x = new `abc${ 1 }def`; `` |
| `es6/templates/templateStringInObjectLiteral.ts` | porter failure | nothing to prune at offsets 67; our first unsupported error: expected field name |
| `es6/templates/templateStringInObjectLiteralES6.ts` | porter failure | nothing to prune at offsets 64; our first unsupported error: expected field name |
| `es6/templates/templateStringInParentheses.ts` | checks too little | 2 after the port |
| `es6/templates/templateStringInParenthesesES6.ts` | duplicate | of `es6/templates/templateStringInParentheses.ts` |
| `es6/templates/templateStringInPropertyAssignment.ts` | checks too little | 3 after the port |
| `es6/templates/templateStringInPropertyAssignmentES6.ts` | duplicate | of `es6/templates/templateStringInPropertyAssignment.ts` |
| `es6/templates/templateStringInPropertyName1.ts` | porter failure | nothing to prune at offsets 42; our first unsupported error: expected field name |
| `es6/templates/templateStringInPropertyName2.ts` | porter failure | nothing to prune at offsets 66; our first unsupported error: expected field name |
| `es6/templates/templateStringInPropertyNameES6_1.ts` | porter failure | nothing to prune at offsets 39; our first unsupported error: expected field name |
| `es6/templates/templateStringInPropertyNameES6_2.ts` | porter failure | nothing to prune at offsets 63; our first unsupported error: expected field name |
| `es6/templates/templateStringInSwitchAndCaseES6.ts` | duplicate | of `es6/templates/templateStringInSwitchAndCase.ts` |
| `es6/templates/templateStringInTaggedTemplate.ts` | not supported | expected `;` after expression, on `` `I AM THE ${ `${ `TAG` } ` } PORTION`    `I ${ "AM" } THE TEMPLATE PORTION` `` |
| `es6/templates/templateStringInTaggedTemplateES6.ts` | not supported | expected `;` after expression, on `` `I AM THE ${ `${ `TAG` } ` } PORTION`    `I ${ "AM" } THE TEMPLATE PORTION` `` |
| `es6/templates/templateStringInTypeAssertion.ts` | not supported | `any` is not supported, on `` let x = <any>`abc${ 123 }def`; `` |
| `es6/templates/templateStringInTypeAssertionES6.ts` | not supported | `any` is not supported, on `` let x = <any>`abc${ 123 }def`; `` |
| `es6/templates/templateStringInTypeOf.ts` | not supported | `typeof` is only valid in the narrowing-guard form `typeof x === "T"`, on `` let x = typeof `abc${ 123 }def`; `` |
| `es6/templates/templateStringInTypeOfES6.ts` | not supported | `typeof` is only valid in the narrowing-guard form `typeof x === "T"`, on `` let x = typeof `abc${ 123 }def`; `` |
| `es6/templates/templateStringInUnaryPlus.ts` | checks too little | 3 after the port |
| `es6/templates/templateStringInUnaryPlusES6.ts` | duplicate | of `es6/templates/templateStringInUnaryPlus.ts` |
| `es6/templates/templateStringInWhile.ts` | checks too little | 2 after the port |
| `es6/templates/templateStringInWhileES6.ts` | duplicate | of `es6/templates/templateStringInWhile.ts` |
| `es6/templates/templateStringInYieldKeyword.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448 |
| `es6/templates/templateStringMultiline1_ES6.ts` | not supported | unknown escape sequence `\ `, on ` \ ` |
| `es6/templates/templateStringMultiline1.ts` | not supported | unknown escape sequence `\ `, on ` \ ` |
| `es6/templates/templateStringMultiline2_ES6.ts` | not supported | unknown escape sequence `\ `, on ` \ ` |
| `es6/templates/templateStringMultiline2.ts` | not supported | unknown escape sequence `\ `, on ` \ ` |
| `es6/templates/templateStringMultiline3_ES6.ts` | not supported | unknown escape sequence `\ `, on ` \ ` |
| `es6/templates/templateStringMultiline3.ts` | not supported | unknown escape sequence `\ `, on ` \ ` |
| `es6/templates/templateStringPlainCharactersThatArePartsOfEscapes01_ES6.ts` | duplicate | of `es6/templates/templateStringPlainCharactersThatArePartsOfEscapes01.ts` |
| `es6/templates/templateStringPlainCharactersThatArePartsOfEscapes01.ts` | checks too little | 0 after the port |
| `es6/templates/templateStringPlainCharactersThatArePartsOfEscapes02_ES6.ts` | duplicate | of `es6/templates/taggedTemplateStringsPlainCharactersThatArePartsOfEscapes02.ts` |
| `es6/templates/templateStringPlainCharactersThatArePartsOfEscapes02.ts` | duplicate | of `es6/templates/taggedTemplateStringsPlainCharactersThatArePartsOfEscapes02.ts` |
| `es6/templates/templateStringsWithTypeErrorInFunctionExpressionsInSubstitutionExpression.ts` | not supported | template-literal interpolation: `.toString()` not supported on `(arg0: number) => void`, on `` `${function (x: number) { x = "bad"; } }`; `` |
| `es6/templates/templateStringsWithTypeErrorInFunctionExpressionsInSubstitutionExpressionES6.ts` | not supported | template-literal interpolation: `.toString()` not supported on `(arg0: number) => void`, on `` `${function (x: number) { x = "bad"; } }`; `` |
| `es6/templates/templateStringTermination1_ES6.ts` | duplicate | of `es6/templates/templateStringTermination1.ts` |
| `es6/templates/templateStringTermination1.ts` | checks too little | 0 after the port |
| `es6/templates/templateStringTermination2_ES6.ts` | duplicate | of `es6/templates/templateStringTermination2.ts` |
| `es6/templates/templateStringTermination2.ts` | checks too little | 0 after the port |
| `es6/templates/templateStringTermination3_ES6.ts` | duplicate | of `es6/templates/templateStringTermination3.ts` |
| `es6/templates/templateStringTermination3.ts` | checks too little | 0 after the port |
| `es6/templates/templateStringTermination4_ES6.ts` | duplicate | of `es6/templates/templateStringTermination4.ts` |
| `es6/templates/templateStringTermination4.ts` | checks too little | 0 after the port |
| `es6/templates/templateStringTermination5_ES6.ts` | duplicate | of `es6/templates/templateStringTermination5.ts` |
| `es6/templates/templateStringTermination5.ts` | checks too little | 0 after the port |
| `es6/templates/templateStringUnterminated1_ES6.ts` | not supported | unterminated template literal, on `` ` `` |
| `es6/templates/templateStringUnterminated1.ts` | not supported | unterminated template literal, on `` ` `` |
| `es6/templates/templateStringUnterminated2_ES6.ts` | not supported | unterminated template literal, on `` `\` `` |
| `es6/templates/templateStringUnterminated2.ts` | not supported | unterminated template literal, on `` `\` `` |
| `es6/templates/templateStringUnterminated3_ES6.ts` | not supported | unterminated template literal, on `` `\\ `` |
| `es6/templates/templateStringUnterminated3.ts` | not supported | unterminated template literal, on `` `\\ `` |
| `es6/templates/templateStringUnterminated4_ES6.ts` | not supported | unterminated template literal, on `` `\\\` `` |
| `es6/templates/templateStringUnterminated4.ts` | not supported | unterminated template literal, on `` `\\\` `` |
| `es6/templates/templateStringUnterminated5_ES6.ts` | not supported | unterminated template literal, on `` `\\\\\` `` |
| `es6/templates/templateStringUnterminated5.ts` | not supported | unterminated template literal, on `` `\\\\\` `` |
| `es6/templates/templateStringWhitespaceEscapes1_ES6.ts` | duplicate | of `es6/templates/templateStringWhitespaceEscapes1.ts` |
| `es6/templates/templateStringWhitespaceEscapes1.ts` | checks too little | 0 after the port |
| `es6/templates/templateStringWhitespaceEscapes2_ES6.ts` | duplicate | of `es6/templates/templateStringWhitespaceEscapes2.ts` |
| `es6/templates/templateStringWhitespaceEscapes2.ts` | checks too little | 0 after the port |
| `es6/templates/templateStringWithBackslashEscapes01_ES6.ts` | not supported | unknown escape sequence `\w`, on `` let a = `hello\world`; `` |
| `es6/templates/templateStringWithBackslashEscapes01.ts` | not supported | unknown escape sequence `\w`, on `` let a = `hello\world`; `` |
| `es6/templates/templateStringWithEmbeddedAddition.ts` | checks too little | 3 after the port |
| `es6/templates/templateStringWithEmbeddedAdditionES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedAddition.ts` |
| `es6/templates/templateStringWithEmbeddedArray.ts` | checks too little | 3 after the port |
| `es6/templates/templateStringWithEmbeddedArrayES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedArray.ts` |
| `es6/templates/templateStringWithEmbeddedArrowFunction.ts` | not supported | template-literal interpolation: `.toString()` not supported on `(arg0: <error>) => <error>`, on `` let x = `abc${ x => x }def`; `` |
| `es6/templates/templateStringWithEmbeddedArrowFunctionES6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/templates/templateStringWithEmbeddedComments.ts` | checks too little | 1 after the port |
| `es6/templates/templateStringWithEmbeddedCommentsES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedComments.ts` |
| `es6/templates/templateStringWithEmbeddedConditional.ts` | not supported | template-literal interpolation: `.toString()` not supported on `string \| boolean`, on `` let x = `abc${ true ? false : " " }def`; `` |
| `es6/templates/templateStringWithEmbeddedConditionalES6.ts` | not supported | template-literal interpolation: `.toString()` not supported on `string \| boolean`, on `` let x = `abc${ true ? false : " " }def`; `` |
| `es6/templates/templateStringWithEmbeddedDivision.ts` | checks too little | 3 after the port |
| `es6/templates/templateStringWithEmbeddedDivisionES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedDivision.ts` |
| `es6/templates/templateStringWithEmbeddedFunctionExpression.ts` | not supported | template-literal interpolation: `.toString()` not supported on `() => () => unknown`, on `` let x = `abc${ function y() { return y; } }def`; `` |
| `es6/templates/templateStringWithEmbeddedFunctionExpressionES6.ts` | not supported | template-literal interpolation: `.toString()` not supported on `() => () => unknown`, on `` let x = `abc${ function y() { return y; } }def`; `` |
| `es6/templates/templateStringWithEmbeddedInOperator.ts` | not supported | expected `,` or `}`, on `` let x = `abc${ "hi" in { hi: 10, hello: 20} }def`; `` |
| `es6/templates/templateStringWithEmbeddedInOperatorES6.ts` | not supported | expected `,` or `}`, on `` let x = `abc${ "hi" in { hi: 10, hello: 20} }def`; `` |
| `es6/templates/templateStringWithEmbeddedInstanceOf.ts` | checks too little | 4 after the port |
| `es6/templates/templateStringWithEmbeddedInstanceOfES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedInstanceOf.ts` |
| `es6/templates/templateStringWithEmbeddedModulo.ts` | checks too little | 3 after the port |
| `es6/templates/templateStringWithEmbeddedModuloES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedModulo.ts` |
| `es6/templates/templateStringWithEmbeddedMultiplication.ts` | checks too little | 3 after the port |
| `es6/templates/templateStringWithEmbeddedMultiplicationES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedMultiplication.ts` |
| `es6/templates/templateStringWithEmbeddedNewOperator.ts` | not supported | `new` expects a constructor; `StringConstructor` declares no `new` method, on `` let x = `abc${ new String("Hi") }def`; `` |
| `es6/templates/templateStringWithEmbeddedNewOperatorES6.ts` | not supported | `new` expects a constructor; `StringConstructor` declares no `new` method, on `` let x = `abc${ new String("Hi") }def`; `` |
| `es6/templates/templateStringWithEmbeddedObjectLiteral.ts` | checks too little | 2 after the port |
| `es6/templates/templateStringWithEmbeddedObjectLiteralES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedObjectLiteral.ts` |
| `es6/templates/templateStringWithEmbeddedTemplateString.ts` | checks too little | 4 after the port |
| `es6/templates/templateStringWithEmbeddedTemplateStringES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedTemplateString.ts` |
| `es6/templates/templateStringWithEmbeddedTypeAssertionOnAddition.ts` | not supported | `any` is not supported, on `` let x = `abc${ <any>(10 + 10) }def`; `` |
| `es6/templates/templateStringWithEmbeddedTypeAssertionOnAdditionES6.ts` | not supported | `any` is not supported, on `` let x = `abc${ <any>(10 + 10) }def`; `` |
| `es6/templates/templateStringWithEmbeddedTypeOfOperator.ts` | not supported | `typeof` is only valid in the narrowing-guard form `typeof x === "T"`, on `` let x = `abc${ typeof "hi" }def`; `` |
| `es6/templates/templateStringWithEmbeddedTypeOfOperatorES6.ts` | not supported | `typeof` is only valid in the narrowing-guard form `typeof x === "T"`, on `` let x = `abc${ typeof "hi" }def`; `` |
| `es6/templates/templateStringWithEmbeddedUnaryPlus.ts` | checks too little | 4 after the port |
| `es6/templates/templateStringWithEmbeddedUnaryPlusES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedUnaryPlus.ts` |
| `es6/templates/templateStringWithEmbeddedYieldKeyword.ts` | not supported | expected function name, on ` function* gen: Generator<number, void, unknown> { ` |
| `es6/templates/templateStringWithEmbeddedYieldKeywordES6.ts` | not supported | expected function name, on ` function* gen(): Generator<number, void, unknown> { ` |
| `es6/templates/templateStringWithEmptyLiteralPortions.ts` | not supported | expected `;` after expression, on `` var c = `1${ 0 }`; `` |
| `es6/templates/templateStringWithEmptyLiteralPortionsES6.ts` | not supported | expected `;` after expression, on `` var c = `1${ 0 }`; `` |
| `es6/templates/templateStringWithOpenCommentInStringPortion.ts` | checks too little | 1 after the port |
| `es6/templates/templateStringWithOpenCommentInStringPortionES6.ts` | duplicate | of `es6/templates/templateStringWithOpenCommentInStringPortion.ts` |
| `es6/templates/templateStringWithPropertyAccess.ts` | checks too little | 2 after the port |
| `es6/templates/templateStringWithPropertyAccessES6.ts` | duplicate | of `es6/templates/templateStringWithPropertyAccess.ts` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions01.ts` | checks too little | 2 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions02.ts` | checks too little | 2 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions03.ts` | checks too little | 2 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions04.ts` | checks too little | 2 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions05.ts` | checks too little | 2 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions06.ts` | checks too little | 2 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions07.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions08.ts` | checks too little | 2 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions09.ts` | checks too little | 2 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions10.ts` | checks too little | 0 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions11.ts` | checks too little | 0 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions12.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions13.ts` | checks too little | 2 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions14.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions15.ts` | checks too little | 2 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions16.ts` | checks too little | 2 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions17.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions18.ts` | checks too little | 2 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions19.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings01.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings02.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings03.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings04.ts` | not supported | invalid code point in `\u{…}`: too many digits, on ` let x = "\u{00000000}"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings05.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings06.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings07.ts` | not supported | invalid code point in `\u{…}`: exceeds U+10FFFF, on ` let x = "\u{110000}"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings08.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings09.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings10.ts` | not supported | invalid code point in `\u{…}`: surrogate, on ` let x = "\u{D800}"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings11.ts` | not supported | invalid code point in `\u{…}`: surrogate, on ` let x = "\u{DC00}"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings12.ts` | not supported | invalid code point in `\u{…}`: too many digits, on ` let x = "\u{FFFFFFFF}"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings13.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings14.ts` | not supported | invalid unicode escape: expected hex digits, on ` let x = "\u{-DDDD}"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings15.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings16.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings17.ts` | not supported | invalid unicode escape: expected hex digits, on ` let x = "\u{r}\u{n}\u{t}"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings18.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings19.ts` | not supported | invalid unicode escape: expected hex digits, on ` let x = "\u{}"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings20.ts` | not supported | invalid unicode escape: expected hex digits, on ` let x = "\u{"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings21.ts` | not supported | invalid unicode escape: expected `}`, on ` let x = "\u{67"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings22.ts` | not supported | invalid unicode escape: expected `}`, on ` let x = "\u{00000000000067"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings23.ts` | not supported | invalid code point in `\u{…}`: too many digits, on ` let x = "\u{00000000000067}"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings24.ts` | not supported | invalid unicode escape: expected `}`, on ` let x = "\u{00000000000067 ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings25.ts` | not supported | invalid code point in `\u{…}`: too many digits, on ` let x = "\u{00000000000067} ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates01.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates02.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates03.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates04.ts` | not supported | invalid code point in `\u{…}`: too many digits, on `` let x = `\u{00000000}`; `` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates05.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates06.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates07.ts` | not supported | invalid code point in `\u{…}`: exceeds U+10FFFF, on `` let x = `\u{110000}`; `` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates08.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates09.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates10.ts` | not supported | invalid code point in `\u{…}`: surrogate, on `` let x = `\u{D800}`; `` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates11.ts` | not supported | invalid code point in `\u{…}`: surrogate, on `` let x = `\u{DC00}`; `` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates12.ts` | not supported | invalid code point in `\u{…}`: too many digits, on `` let x = `\u{FFFFFFFF}`; `` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates13.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates14.ts` | not supported | invalid unicode escape: expected hex digits, on `` let x = `\u{-DDDD}`; `` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates15.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates16.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates17.ts` | not supported | invalid unicode escape: expected hex digits, on `` let x = `\u{r}\u{n}\u{t}`; `` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates18.ts` | checks too little | 1 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates19.ts` | not supported | invalid unicode escape: expected hex digits, on `` let x = `\u{}`; `` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates20.ts` | checks too little | 2 after the port |
| `es6/variableDeclarations/VariableDeclaration1_es6.ts` | porter failure | nothing to prune at offsets 22; our first unsupported error: expected identifier after `let`/`const` |
| `es6/variableDeclarations/VariableDeclaration10_es6.ts` | checks too little | 1 after the port |
| `es6/variableDeclarations/VariableDeclaration11_es6.ts` | porter failure | nothing to prune at offsets 34; our first unsupported error: expected identifier after `let`/`const` |
| `es6/variableDeclarations/VariableDeclaration12_es6.ts` | not supported | expected identifier after `let`/`const`, on ` x ` |
| `es6/variableDeclarations/VariableDeclaration13_es6.ts` | porter failure | nothing to prune at offsets 264; our first unsupported error: `let` is a reserved keyword and can't be used as a name |
| `es6/variableDeclarations/VariableDeclaration2_es6.ts` | the port changes what it checks | `tsc` then reports TS7005 |
| `es6/variableDeclarations/VariableDeclaration3_es6.ts` | checks too little | 1 after the port |
| `es6/variableDeclarations/VariableDeclaration4_es6.ts` | checks too little | 3 after the port |
| `es6/variableDeclarations/VariableDeclaration5_es6.ts` | checks too little | 1 after the port |
| `es6/variableDeclarations/VariableDeclaration6_es6.ts` | porter failure | nothing to prune at offsets 20; our first unsupported error: expected identifier after `let`/`const` |
| `es6/variableDeclarations/VariableDeclaration7_es6.ts` | porter failure | nothing to prune at offsets 22; our first unsupported error: `let` declaration requires an initializer |
| `es6/variableDeclarations/VariableDeclaration8_es6.ts` | checks too little | 1 after the port |
| `es6/variableDeclarations/VariableDeclaration9_es6.ts` | checks too little | 3 after the port |
| `es6/yieldExpressions/` | not supported | generators |
| `es7/exponentiationOperator/compoundExponentiationAssignmentLHSCanBeAssigned1.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es7/exponentiationOperator/compoundExponentiationAssignmentLHSIsReference.ts` | not supported | `any` is not supported, on ` let value: any = null as unknown as (any); ` |
| `es7/exponentiationOperator/compoundExponentiationAssignmentLHSIsValue.ts` | the port changes what it checks | `tsc` then reports TS2683, TS7005 |
| `es7/exponentiationOperator/emitCompoundExponentiationAssignmentWithIndexingOnLHS2.ts` | not supported | expected field name in object type, on ` function foo(): { 0: number; } { ` |
| `es7/exponentiationOperator/emitCompoundExponentiationAssignmentWithIndexingOnLHS3.ts` | not supported | expected `,` or `}`, on ` get 0() { ` |
| `es7/exponentiationOperator/emitExponentiationOperator4.ts` | not supported | unexpected character `~`, on ` (~ --temp) ** 3; ` |
| `es7/exponentiationOperator/emitExponentiationOperatorInTempalteString4ES6.ts` | duplicate | of `es7/exponentiationOperator/emitExponentiationOperatorInTempalteString4.ts` |
| `es7/exponentiationOperator/emitExponentiationOperatorInTemplateString1ES6.ts` | duplicate | of `es7/exponentiationOperator/emitExponentiationOperatorInTemplateString1.ts` |
| `es7/exponentiationOperator/emitExponentiationOperatorInTemplateString2ES6.ts` | duplicate | of `es7/exponentiationOperator/emitExponentiationOperatorInTemplateString2.ts` |
| `es7/exponentiationOperator/emitExponentiationOperatorInTemplateString3ES6.ts` | duplicate | of `es7/exponentiationOperator/emitExponentiationOperatorInTemplateString3.ts` |
| `es7/exponentiationOperator/exponentiationOperatorSyntaxError2.ts` | not supported | unexpected character `~`, on ` ~ --temp ** 3; ` |
| `es7/exponentiationOperator/exponentiationOperatorWithAnyAndNumber.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es7/exponentiationOperator/exponentiationOperatorWithInvalidSimpleUnaryExpressionOperands.ts` | not supported | `any` is not supported, on ` let temp: any = null as unknown as (any); ` |
| `es7/exponentiationOperator/exponentiationOperatorWithNew.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `es7/exponentiationOperator/exponentiationOperatorWithOnlyNullValueOrUndefinedValue.ts` | checks too little | 4 after the port |
| `es7/exponentiationOperator/exponentiationOperatorWithTemplateStringInvalid.ts` | not supported | expected `;` after expression, on `` var b = 1 ** `2${ 3 }`; `` |
| `es7/exponentiationOperator/exponentiationOperatorWithTemplateStringInvalidES6.ts` | not supported | expected `;` after expression, on `` var b = 1 ** `2${ 3 }`; `` |
| `es7/exponentiationOperator/exponentiationOperatorWithTypeParameter.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `es7/exponentiationOperator/exponentiationOperatorWithUndefinedValueAndInvalidOperands.ts` | duplicate | of `es7/exponentiationOperator/exponentiationOperatorWithNullValueAndInvalidOperands.ts` |
| `es7/trailingCommasInBindingPatterns.ts` | not supported | expected expression, on ` const {...b,} = {}; ` |
| `es7/trailingCommasInFunctionParametersAndArguments.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `es7/trailingCommasInGetter.ts` | not supported | expected parameter name, on ` get x(,) { return 0; } ` |
| `esDecorators/` | not supported | decorators |
| `esnext/esnextSharedMemory.ts` | the port changes what it checks | `tsc` then reports TS2550 |
| `esnext/logicalAssignment/logicalAssignment11.ts` | not supported | expected expression, on ` e ??= x ?? "x" ` |
| `expressions/arrayLiterals/arrayLiterals.ts` | the port changes what it checks | `tsc` then reports TS7008 |
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
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNoRelationshipPrimitiveType.ts` | not supported | cannot cast `unknown` to `void`: no assignable direction between these types, on ` let d: void = null as unknown as (void); ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNumberOperand.ts` | not supported | unknown type `Promise`, on ` const t1: number \| Promise<number> = null as unknown as (number \| Promise<num... ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNumericLiteral.ts` | not supported | unexpected character `&`, on ` type BrandedNum = number & { __numberBrand: any }; ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithOneOperandIsAny.ts` | not supported | `any` is not supported, on ` let x: any = null as unknown as (any); ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithOneOperandIsUndefined.ts` | the port changes what it checks | `tsc` then reports TS2304 |
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
| `expressions/contextualTyping/generatedContextualTyping.ts` | the port changes what it checks | `tsc` then reports TS7008, TS7010, TS2352, TS2322 |
| `expressions/contextualTyping/getSetAccessorContextualTyping.ts` | not supported | parameter requires a type annotation, on ` set Y(y) { } ` |
| `expressions/contextualTyping/iterableContextualTyping1.ts` | not supported | parameter `s` requires a type annotation, on ` let iter: Iterable<(x: string) => number> = [s => s.length]; ` |
| `expressions/contextualTyping/objectLiteralContextualTyping.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
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
| `expressions/functionCalls/overloadResolution.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `expressions/functionCalls/overloadResolutionClassConstructors.ts` | the port changes what it checks | `tsc` then reports TS7008, TS2322, TS2409 |
| `expressions/functionCalls/overloadResolutionConstructors.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `expressions/functionCalls/typeArgumentInference.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `expressions/functionCalls/typeArgumentInferenceConstructSignatures.ts` | not supported | construct signatures |
| `expressions/functionCalls/typeArgumentInferenceTransitiveConstraints.ts` | not supported | expected `,` or `>`, on ` function fn<A extends Date, B extends A, C extends B>(a: A, b: B, c: C): A[] { ` |
| `expressions/functionCalls/typeArgumentInferenceWithConstraints.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `expressions/functionCalls/typeArgumentInferenceWithObjectLiteral.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `expressions/functions/arrowFunctionContexts.ts` | the port changes what it checks | `tsc` then reports TS2683, TS7006 |
| `expressions/functions/arrowFunctionExpressions.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7031, TS2683 |
| `expressions/functions/contextuallyTypedIife.ts` | the port changes what it checks | `tsc` then reports TS1359, TS18048, TS7006 |
| `expressions/functions/contextuallyTypedIifeStrict.ts` | the port changes what it checks | `tsc` then reports TS1359 |
| `expressions/functions/voidParamAssignmentCompatibility.ts` | not supported | `void` cannot be a parameter type — it has no values, on ` function g(a: void): void { } ` |
| `expressions/identifiers/scopeResolutionIdentifiers.ts` | not supported | expected `;` after expression, on ` namespace M1 { ` |
| `expressions/literals/strictModeOctalLiterals.ts` | not supported | expected `,` or `}` after enum member, on ` A = 12 + 01 ` |
| `expressions/newOperator/newOperatorConformance.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `expressions/newOperator/newOperatorErrorCases_noImplicitAny.ts` | not supported | `this` is a reserved keyword and can't be used as a name, on ` function fnNumber(this: void): number { return 90; } ` |
| `expressions/newOperator/newOperatorErrorCases.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `expressions/nullishCoalescingOperator/nullishCoalescingAssignmentVsPrivateFieldsJsEmit1.ts` | not supported | unexpected character `#`, on ` #privateProp: number \| null; ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperator_es2020.ts` | duplicate | of `expressions/nullishCoalescingOperator/nullishCoalescingOperator2.ts` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperator_not_strict.ts` | duplicate | of `expressions/nullishCoalescingOperator/nullishCoalescingOperator2.ts` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperator12.ts` | not supported | `any` is not supported, on ` const obj: { arr: any[] } = { arr: [] }; ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperator6.ts` | not supported | default value must be a literal or enum variant, on ` function foo(foo: string, bar: string = foo ?? "bar"): void { } ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperator9.ts` | not supported | expected expression, on ` let g = f \|\| (abc => { void abc.toLowerCase() }) ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperatorInAsyncGenerator.ts` | not supported | expected `;` after expression, on ` async function* f(a: { b?: number }): AsyncGenerator<number, void, unknown> { ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperatorInParameterBindingPattern.2.ts` | the port changes what it checks | `tsc` then reports TS2537, TS2339 |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperatorInParameterBindingPattern.ts` | the port changes what it checks | `tsc` then reports TS2537 |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperatorInParameterInitializer.2.ts` | not supported | default parameter values are only supported on function declarations, on ` ((b: string = a() ?? "d") => { let a; })(); ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperatorInParameterInitializer.ts` | not supported | default parameter values are only supported on function declarations, on ` ((b: string = a() ?? "d") => {})(); ` |
| `expressions/objectLiterals/objectLiteralErrors.ts` | not supported | unexpected character `#`, on ` #z: 3 ` |
| `expressions/objectLiterals/objectLiteralGettersAndSetters.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7032, TS7006 |
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
| `expressions/optionalChaining/optionalChainingInParameterInitializer.2.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `expressions/optionalChaining/optionalChainingInParameterInitializer.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `expressions/optionalChaining/optionalChainingInTypeAssertions.ts` | not supported | `any` is not supported, on ` (foo.m as any)?.(); ` |
| `expressions/optionalChaining/privateIdentifierChain/` | not supported | private `#names` |
| `expressions/optionalChaining/propertyAccessChain/propertyAccessChain.3.ts` | not supported | `any` is not supported, on ` const obj: any = null as unknown as (any); ` |
| `expressions/optionalChaining/taggedTemplateChain/` | not supported | tagged templates |
| `expressions/propertyAccess/propertyAccess.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `expressions/propertyAccess/propertyAccessWidening.ts` | not supported | `any` is not supported, on ` function g1(headerNames: any): void { ` |
| `expressions/superCalls/errorSuperCalls.ts` | not supported | parameter requires a type annotation, on ` set foo(v) { ` |
| `expressions/superCalls/superCalls.ts` | not supported | cannot bind a `void` value, on ` let p = super(''); ` |
| `expressions/superPropertyAccess/errorSuperPropertyAccess.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `expressions/superPropertyAccess/superPropertyAccessNoError.ts` | not supported | expected type, on ` returnThis(): this { ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess1.ts` | not supported | expected class member name, on ` [symbol](): number { ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess2.ts` | not supported | expected class member name, on ` [Symbol.isConcatSpreadable](): number { ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess3.ts` | not supported | expected class member name, on ` [symbol](): number { ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess4.ts` | not supported | expected class member name, on ` [symbol](): any { ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess5.ts` | not supported | `any` is not supported, on ` let symbol: any = null as unknown as (any); ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess6.ts` | not supported | `any` is not supported, on ` let symbol: any = null as unknown as (any); ` |
| `expressions/thisKeyword/thisInInvalidContexts.ts` | the port changes what it checks | `tsc` then reports TS7008, TS2683 |
| `expressions/thisKeyword/thisInInvalidContextsExternalModule.ts` | the port changes what it checks | `tsc` then reports TS7008, TS2683, TS1203 |
| `expressions/thisKeyword/typeOfThisGeneral.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `expressions/thisKeyword/typeOfThisInConstructorParamList.ts` | not supported | parameter requires a type annotation, on ` constructor(f = this) { } ` |
| `expressions/typeAssertions/constAssertionOnEnum.ts` | multi-file or JavaScript |  |
| `expressions/typeAssertions/constAssertions.ts` | not supported | expected type, on ` let v1 = 'abc' as const; ` |
| `expressions/typeAssertions/typeAssertions.ts` | the port changes what it checks | `tsc` then reports TS7008 |
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
| `expressions/unaryOperators/negateOperator/negateOperatorInvalidOperations.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
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
| `externalModules/amdImportAsPrimaryExpression.ts` | multi-file or JavaScript |  |
| `externalModules/amdImportNotAsPrimaryExpression.ts` | multi-file or JavaScript |  |
| `externalModules/asiPreventsParsingAsAmbientExternalModule02.ts` | not supported | expected `;` after expression, on ` namespace container { ` |
| `externalModules/circularReference.ts` | multi-file or JavaScript |  |
| `externalModules/commonJSImportAsPrimaryExpression.ts` | multi-file or JavaScript |  |
| `externalModules/commonJsImportBindingElementNarrowType.ts` | multi-file or JavaScript |  |
| `externalModules/commonJSImportNotAsPrimaryExpression.ts` | multi-file or JavaScript |  |
| `externalModules/duplicateExportAssignments.ts` | multi-file or JavaScript |  |
| `externalModules/es6/es6modulekind.ts` | not supported | `export default` is not supported, on ` export default class A ` |
| `externalModules/es6/es6modulekindExportClassNameWithObject.ts` | checks too little | 0 after the port |
| `externalModules/es6/es6modulekindWithES2015Target.ts` | not supported | `export default` is not supported, on ` export default class A ` |
| `externalModules/es6/es6modulekindWithES5Target.ts` | not supported | unexpected character `@`, on ` @foo ` |
| `externalModules/es6/es6modulekindWithES5Target10.ts` | the port changes what it checks | `tsc` then reports TS2792 |
| `externalModules/es6/es6modulekindWithES5Target11.ts` | not supported | unexpected character `@`, on ` @foo ` |
| `externalModules/es6/es6modulekindWithES5Target12.ts` | not supported | expected a declaration after `export`, on ` export namespace C { ` |
| `externalModules/es6/es6modulekindWithES5Target2.ts` | not supported | `export default` is not supported, on ` export default class C { ` |
| `externalModules/es6/es6modulekindWithES5Target3.ts` | not supported | unexpected character `@`, on ` @foo ` |
| `externalModules/es6/es6modulekindWithES5Target4.ts` | not supported | `export default` is not supported, on ` export default E; ` |
| `externalModules/es6/es6modulekindWithES5Target5.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` export const enum E2 { ` |
| `externalModules/es6/es6modulekindWithES5Target6.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `externalModules/es6/es6modulekindWithES5Target7.ts` | not supported | expected a declaration after `export`, on ` export namespace N { ` |
| `externalModules/es6/es6modulekindWithES5Target8.ts` | checks too little | 2 after the port |
| `externalModules/es6/es6modulekindWithES5Target9.ts` | the port changes what it checks | `tsc` then reports TS2792 |
| `externalModules/esnext/esnextmodulekind.ts` | not supported | `export default` is not supported, on ` export default class A ` |
| `externalModules/esnext/esnextmodulekindWithES2015Target.ts` | not supported | `export default` is not supported, on ` export default class A ` |
| `externalModules/esnext/esnextmodulekindWithES5Target.ts` | duplicate | of `externalModules/es6/es6modulekindWithES5Target.ts` |
| `externalModules/esnext/esnextmodulekindWithES5Target10.ts` | the port changes what it checks | `tsc` then reports TS2792 |
| `externalModules/esnext/esnextmodulekindWithES5Target11.ts` | not supported | unexpected character `@`, on ` @foo ` |
| `externalModules/esnext/esnextmodulekindWithES5Target12.ts` | duplicate | of `externalModules/es6/es6modulekindWithES5Target12.ts` |
| `externalModules/esnext/esnextmodulekindWithES5Target2.ts` | not supported | `export default` is not supported, on ` export default class C { ` |
| `externalModules/esnext/esnextmodulekindWithES5Target3.ts` | not supported | unexpected character `@`, on ` @foo ` |
| `externalModules/esnext/esnextmodulekindWithES5Target4.ts` | not supported | `export default` is not supported, on ` export default E; ` |
| `externalModules/esnext/esnextmodulekindWithES5Target5.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` export const enum E2 { ` |
| `externalModules/esnext/esnextmodulekindWithES5Target6.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `externalModules/esnext/esnextmodulekindWithES5Target7.ts` | not supported | expected a declaration after `export`, on ` export namespace N { ` |
| `externalModules/esnext/esnextmodulekindWithES5Target8.ts` | duplicate | of `externalModules/es6/es6modulekindWithES5Target8.ts` |
| `externalModules/esnext/esnextmodulekindWithES5Target9.ts` | the port changes what it checks | `tsc` then reports TS2792 |
| `externalModules/esnext/exnextmodulekindExportClassNameWithObject.ts` | duplicate | of `externalModules/es6/es6modulekindExportClassNameWithObject.ts` |
| `externalModules/exportAmbientClassNameWithObject.ts` | not supported | expected a declaration after `export`, on ` export declare class Object {} ` |
| `externalModules/exportAssignDottedName.ts` | multi-file or JavaScript |  |
| `externalModules/exportAssignImportedIdentifier.ts` | multi-file or JavaScript |  |
| `externalModules/exportAssignmentAndDeclaration.ts` | multi-file or JavaScript |  |
| `externalModules/exportAssignmentCircularModules.ts` | multi-file or JavaScript |  |
| `externalModules/exportAssignmentConstrainedGenericType.ts` | multi-file or JavaScript |  |
| `externalModules/exportAssignmentGenericType.ts` | multi-file or JavaScript |  |
| `externalModules/exportAssignmentMergedInterface.ts` | multi-file or JavaScript |  |
| `externalModules/exportAssignmentMergedModule.ts` | multi-file or JavaScript |  |
| `externalModules/exportAssignmentOfExportNamespaceWithDefault.ts` | multi-file or JavaScript |  |
| `externalModules/exportAssignmentTopLevelClodule.ts` | multi-file or JavaScript |  |
| `externalModules/exportAssignmentTopLevelEnumdule.ts` | multi-file or JavaScript |  |
| `externalModules/exportAssignmentTopLevelFundule.ts` | multi-file or JavaScript |  |
| `externalModules/exportAssignmentTopLevelIdentifier.ts` | multi-file or JavaScript |  |
| `externalModules/exportAssignNonIdentifier.ts` | multi-file or JavaScript |  |
| `externalModules/exportAssignTypes.ts` | multi-file or JavaScript |  |
| `externalModules/exportClassNameWithObjectAMD.ts` | duplicate | of `externalModules/es6/es6modulekindExportClassNameWithObject.ts` |
| `externalModules/exportClassNameWithObjectCommonJS.ts` | duplicate | of `externalModules/es6/es6modulekindExportClassNameWithObject.ts` |
| `externalModules/exportClassNameWithObjectSystem.ts` | duplicate | of `externalModules/es6/es6modulekindExportClassNameWithObject.ts` |
| `externalModules/exportClassNameWithObjectUMD.ts` | duplicate | of `externalModules/es6/es6modulekindExportClassNameWithObject.ts` |
| `externalModules/exportDeclaredModule.ts` | multi-file or JavaScript |  |
| `externalModules/exportDefaultClassNameWithObject.ts` | not supported | `export default` is not supported, on ` export default class Object {} ` |
| `externalModules/exportNonInitializedVariablesAMD.ts` | the port changes what it checks | `tsc` then reports TS7005 |
| `externalModules/exportNonInitializedVariablesCommonJS.ts` | the port changes what it checks | `tsc` then reports TS7005 |
| `externalModules/exportNonInitializedVariablesES6.ts` | the port changes what it checks | `tsc` then reports TS7005 |
| `externalModules/exportNonInitializedVariablesInIfThenStatementNoCrash1.ts` | the port changes what it checks | `tsc` then reports TS1156 |
| `externalModules/exportNonInitializedVariablesSystem.ts` | the port changes what it checks | `tsc` then reports TS7005 |
| `externalModules/exportNonInitializedVariablesUMD.ts` | the port changes what it checks | `tsc` then reports TS7005 |
| `externalModules/exportNonLocalDeclarations.ts` | checks too little | 2 after the port |
| `externalModules/exportNonVisibleType.ts` | multi-file or JavaScript |  |
| `externalModules/exportTypeMergedWithExportStarAsNamespace.ts` | multi-file or JavaScript |  |
| `externalModules/globalAugmentationModuleResolution.ts` | multi-file or JavaScript |  |
| `externalModules/importImportOnlyModule.ts` | multi-file or JavaScript |  |
| `externalModules/importNonExternalModule.ts` | multi-file or JavaScript |  |
| `externalModules/importNonStringLiteral.ts` | multi-file or JavaScript |  |
| `externalModules/importsImplicitlyReadonly.ts` | multi-file or JavaScript |  |
| `externalModules/importTsBeforeDTs.ts` | multi-file or JavaScript |  |
| `externalModules/initializersInDeclarations.ts` | multi-file or JavaScript |  |
| `externalModules/invalidSyntaxNamespaceImportWithAMD.ts` | multi-file or JavaScript |  |
| `externalModules/invalidSyntaxNamespaceImportWithCommonjs.ts` | multi-file or JavaScript |  |
| `externalModules/invalidSyntaxNamespaceImportWithSystem.ts` | multi-file or JavaScript |  |
| `externalModules/moduleResolutionWithExtensions.ts` | multi-file or JavaScript |  |
| `externalModules/moduleResolutionWithoutExtension1.ts` | multi-file or JavaScript |  |
| `externalModules/moduleResolutionWithoutExtension2.ts` | multi-file or JavaScript |  |
| `externalModules/moduleResolutionWithoutExtension3.ts` | multi-file or JavaScript |  |
| `externalModules/moduleResolutionWithoutExtension4.ts` | multi-file or JavaScript |  |
| `externalModules/moduleResolutionWithoutExtension5.ts` | multi-file or JavaScript |  |
| `externalModules/moduleResolutionWithoutExtension6.ts` | multi-file or JavaScript |  |
| `externalModules/moduleResolutionWithoutExtension7.ts` | multi-file or JavaScript |  |
| `externalModules/moduleResolutionWithoutExtension8.ts` | multi-file or JavaScript |  |
| `externalModules/moduleScoping.ts` | multi-file or JavaScript |  |
| `externalModules/multipleExportDefault1.ts` | not supported | `export default` is not supported, on ` export default function Foo (): void { ` |
| `externalModules/multipleExportDefault2.ts` | not supported | `export default` is not supported, on ` export default { ` |
| `externalModules/multipleExportDefault3.ts` | not supported | `export default` is not supported, on ` export default { ` |
| `externalModules/multipleExportDefault4.ts` | not supported | `export default` is not supported, on ` export default class C { } ` |
| `externalModules/multipleExportDefault5.ts` | not supported | `export default` is not supported, on ` export default function bar(): void { } ` |
| `externalModules/multipleExportDefault6.ts` | not supported | `export default` is not supported, on ` export default { ` |
| `externalModules/nameDelimitedBySlashes.ts` | multi-file or JavaScript |  |
| `externalModules/nameWithFileExtension.ts` | multi-file or JavaScript |  |
| `externalModules/nameWithRelativePaths.ts` | multi-file or JavaScript |  |
| `externalModules/reexportClassDefinition.ts` | multi-file or JavaScript |  |
| `externalModules/relativePathMustResolve.ts` | multi-file or JavaScript |  |
| `externalModules/relativePathToDeclarationFile.ts` | multi-file or JavaScript |  |
| `externalModules/rewriteRelativeImportExtensions/cjsErrors.ts` | multi-file or JavaScript |  |
| `externalModules/rewriteRelativeImportExtensions/emit.ts` | multi-file or JavaScript |  |
| `externalModules/rewriteRelativeImportExtensions/emitModuleCommonJS.ts` | multi-file or JavaScript |  |
| `externalModules/rewriteRelativeImportExtensions/nodeModulesTsFiles.ts` | multi-file or JavaScript |  |
| `externalModules/rewriteRelativeImportExtensions/nonTSExtensions.ts` | multi-file or JavaScript |  |
| `externalModules/rewriteRelativeImportExtensions/packageJsonImportsErrors.ts` | multi-file or JavaScript |  |
| `externalModules/topLevelAmbientModule.ts` | multi-file or JavaScript |  |
| `externalModules/topLevelAwait.1.ts` | multi-file or JavaScript |  |
| `externalModules/topLevelAwait.2.ts` | the port changes what it checks | `tsc` then reports TS1039 |
| `externalModules/topLevelAwait.3.ts` | multi-file or JavaScript |  |
| `externalModules/topLevelAwaitErrors.1.ts` | the port changes what it checks | `tsc` then reports TS7031 |
| `externalModules/topLevelAwaitErrors.10.ts` | multi-file or JavaScript |  |
| `externalModules/topLevelAwaitErrors.11.ts` | multi-file or JavaScript |  |
| `externalModules/topLevelAwaitErrors.12.ts` | the port changes what it checks | `tsc` then reports TS1039 |
| `externalModules/topLevelAwaitErrors.2.ts` | not supported | empty export specifier list, on ` export {}; ` |
| `externalModules/topLevelAwaitErrors.3.ts` | not supported | empty export specifier list, on ` export {}; ` |
| `externalModules/topLevelAwaitErrors.4.ts` | not supported | empty export specifier list, on ` export {}; ` |
| `externalModules/topLevelAwaitErrors.5.ts` | checks too little | 1 after the port |
| `externalModules/topLevelAwaitErrors.6.ts` | checks too little | 1 after the port |
| `externalModules/topLevelAwaitErrors.7.ts` | multi-file or JavaScript |  |
| `externalModules/topLevelAwaitErrors.8.ts` | multi-file or JavaScript |  |
| `externalModules/topLevelAwaitErrors.9.ts` | multi-file or JavaScript |  |
| `externalModules/topLevelAwaitNonModule.ts` | the port changes what it checks | `tsc` then reports TS1378, TS1432 |
| `externalModules/topLevelFileModule.ts` | multi-file or JavaScript |  |
| `externalModules/topLevelFileModuleMissing.ts` | multi-file or JavaScript |  |
| `externalModules/topLevelModuleDeclarationAndFile.ts` | multi-file or JavaScript |  |
| `externalModules/typeAndNamespaceExportMerge.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/allowsImportingTsExtension.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/ambient.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/chained.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/chained2.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/circular1.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/circular2.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/circular3.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/circular4.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/cjsImportInES2015.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/computedPropertyName.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/enums.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportDeclaration_missingBraces.ts` | not supported | expected `;` after expression, on ` namespace ns { ` |
| `externalModules/typeOnly/exportDeclaration_moduleSpecifier-isolatedModules.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportDeclaration_moduleSpecifier.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportDeclaration_value.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportDeclaration.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportDefault.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportNamespace_js.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportNamespace1.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportNamespace10.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportNamespace11.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportNamespace12.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportNamespace2.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportNamespace3.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportNamespace4.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportNamespace5.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportNamespace6.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportNamespace7.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportNamespace8.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportNamespace9.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportSpecifiers_js.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/exportSpecifiers.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/extendsClause.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/filterNamespace_import.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/generic.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/grammarErrors.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/implementsClause.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/importClause_default.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/importClause_namedImports.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/importClause_namespaceImport.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/importDefaultNamedType.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/importDefaultNamedType2.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/importDefaultNamedType3.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/importEquals1.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/importEquals2.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/importEquals3.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/importEqualsDeclaration.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/importsNotUsedAsValues_error.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/importSpecifiers_js.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/importSpecifiers1.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/mergedWithLocalValue.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/namespaceImportTypeQuery.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/namespaceImportTypeQuery2.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/namespaceImportTypeQuery3.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/namespaceImportTypeQuery4.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/namespaceMemberAccess.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/nestedNamespace.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/preserveValueImports_errors.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/preserveValueImports_importsNotUsedAsValues.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/preserveValueImports_mixedImports.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/preserveValueImports_module.ts` | not supported | empty export specifier list, on ` export {}; ` |
| `externalModules/typeOnly/preserveValueImports.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/renamed.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/typeOnlyESMImportFromCJS.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnly/typeQuery.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnlyMerge1.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnlyMerge2.ts` | multi-file or JavaScript |  |
| `externalModules/typeOnlyMerge3.ts` | multi-file or JavaScript |  |
| `externalModules/typesOnlyExternalModuleStillHasInstance.ts` | multi-file or JavaScript |  |
| `externalModules/typeValueMerge1.ts` | multi-file or JavaScript |  |
| `externalModules/umd-augmentation-1.ts` | multi-file or JavaScript |  |
| `externalModules/umd-augmentation-2.ts` | multi-file or JavaScript |  |
| `externalModules/umd-augmentation-3.ts` | multi-file or JavaScript |  |
| `externalModules/umd-augmentation-4.ts` | multi-file or JavaScript |  |
| `externalModules/umd-errors.ts` | multi-file or JavaScript |  |
| `externalModules/umd1.ts` | multi-file or JavaScript |  |
| `externalModules/umd2.ts` | multi-file or JavaScript |  |
| `externalModules/umd3.ts` | multi-file or JavaScript |  |
| `externalModules/umd4.ts` | multi-file or JavaScript |  |
| `externalModules/umd5.ts` | multi-file or JavaScript |  |
| `externalModules/umd6.ts` | multi-file or JavaScript |  |
| `externalModules/umd7.ts` | multi-file or JavaScript |  |
| `externalModules/umd8.ts` | multi-file or JavaScript |  |
| `externalModules/umd9.ts` | multi-file or JavaScript |  |
| `externalModules/valuesMergingAcrossModules.ts` | multi-file or JavaScript |  |
| `externalModules/verbatimModuleSyntaxAmbientConstEnum.ts` | multi-file or JavaScript |  |
| `externalModules/verbatimModuleSyntaxCompat.ts` | not supported | empty export specifier list, on ` export {}; ` |
| `externalModules/verbatimModuleSyntaxCompat2.ts` | multi-file or JavaScript |  |
| `externalModules/verbatimModuleSyntaxCompat3.ts` | multi-file or JavaScript |  |
| `externalModules/verbatimModuleSyntaxCompat4.ts` | multi-file or JavaScript |  |
| `externalModules/verbatimModuleSyntaxConstEnum.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` export const enum E { ` |
| `externalModules/verbatimModuleSyntaxConstEnumUsage.ts` | multi-file or JavaScript |  |
| `externalModules/verbatimModuleSyntaxDeclarationFile.ts` | multi-file or JavaScript |  |
| `externalModules/verbatimModuleSyntaxInternalImportEquals.ts` | not supported | empty export specifier list, on ` export {}; ` |
| `externalModules/verbatimModuleSyntaxNoElisionCJS.ts` | multi-file or JavaScript |  |
| `externalModules/verbatimModuleSyntaxNoElisionESM.ts` | multi-file or JavaScript |  |
| `externalModules/verbatimModuleSyntaxRestrictionsCJS.ts` | multi-file or JavaScript |  |
| `externalModules/verbatimModuleSyntaxRestrictionsESM.ts` | multi-file or JavaScript |  |
| `fixSignatureCaching.ts` | the port changes what it checks | `tsc` then reports TS1359, TS7006, TS2683, TS2322, TS2580 |
| `functions/functionImplementationErrors.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7008 |
| `functions/functionImplementations.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `functions/functionNameConflicts.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `functions/functionOverloadCompatibilityWithVoid01.ts` | not supported | expected `{`, on ` function f(x: string): number; ` |
| `functions/functionOverloadCompatibilityWithVoid02.ts` | not supported | expected `{`, on ` function f(x: string): void; ` |
| `functions/functionOverloadCompatibilityWithVoid03.ts` | not supported | expected `{`, on ` function f(x: string): void; ` |
| `functions/functionOverloadErrors.ts` | the port changes what it checks | `tsc` then reports TS7010, TS2393 |
| `functions/functionOverloadErrorsSyntax.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `functions/functionParameterObjectRestAndInitializers.ts` | the port changes what it checks | `tsc` then reports TS7031 |
| `functions/functionWithUseStrictAndSimpleParameterList_es2016.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `functions/functionWithUseStrictAndSimpleParameterList.ts` | the port changes what it checks | `tsc` then reports TS1346, TS1347, TS7019 |
| `functions/parameterInitializersBackwardReferencing.ts` | the port changes what it checks | `tsc` then reports TS7022 |
| `functions/parameterInitializersForwardReferencing.2.ts` | not supported | `any` is not supported, on ` function a(): any {} ` |
| `functions/parameterInitializersForwardReferencing.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7022, TS2322, TS7024 |
| `functions/parameterInitializersForwardReferencing1_es6.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7022 |
| `functions/parameterInitializersForwardReferencing1.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7022 |
| `functions/strictBindCallApply1.ts` | the port changes what it checks | `tsc` then reports TS2393 |
| `functions/strictBindCallApply2.ts` | not supported | `this` is a reserved keyword and can't be used as a name, on ` function fn(this: Foo): void {} ` |
| `generators/` | not supported | generators |
| `importAssertion/importAssertion1.ts` | multi-file or JavaScript |  |
| `importAssertion/importAssertion2.ts` | multi-file or JavaScript |  |
| `importAssertion/importAssertion3.ts` | multi-file or JavaScript |  |
| `importAssertion/importAssertion4.ts` | the port changes what it checks | `tsc` then reports TS2792 |
| `importAssertion/importAssertion5.ts` | the port changes what it checks | `tsc` then reports TS2792, TS2322, TS2304, TS2858 |
| `importAttributes/importAttributes1.ts` | multi-file or JavaScript |  |
| `importAttributes/importAttributes10.ts` | multi-file or JavaScript |  |
| `importAttributes/importAttributes11.ts` | multi-file or JavaScript |  |
| `importAttributes/importAttributes2.ts` | multi-file or JavaScript |  |
| `importAttributes/importAttributes3.ts` | multi-file or JavaScript |  |
| `importAttributes/importAttributes4.ts` | the port changes what it checks | `tsc` then reports TS2792 |
| `importAttributes/importAttributes5.ts` | the port changes what it checks | `tsc` then reports TS2792, TS2322, TS2304, TS2858 |
| `importAttributes/importAttributes6.ts` | multi-file or JavaScript |  |
| `importAttributes/importAttributes7.ts` | multi-file or JavaScript |  |
| `importAttributes/importAttributes8.ts` | multi-file or JavaScript |  |
| `importAttributes/importAttributes9.ts` | multi-file or JavaScript |  |
| `importDefer/dynamicImportDefer.ts` | multi-file or JavaScript |  |
| `importDefer/dynamicImportDeferInvalidStandalone.ts` | multi-file or JavaScript |  |
| `importDefer/exportDeferInvalid.ts` | multi-file or JavaScript |  |
| `importDefer/importBindingDefer.ts` | multi-file or JavaScript |  |
| `importDefer/importBindingDefer2.ts` | multi-file or JavaScript |  |
| `importDefer/importDefaultBindingDefer.ts` | multi-file or JavaScript |  |
| `importDefer/importDeferComments.ts` | multi-file or JavaScript |  |
| `importDefer/importDeferDeclaration.ts` | multi-file or JavaScript |  |
| `importDefer/importDeferFromInvalid.ts` | multi-file or JavaScript |  |
| `importDefer/importDeferInvalidDefault.ts` | multi-file or JavaScript |  |
| `importDefer/importDeferInvalidNamed.ts` | multi-file or JavaScript |  |
| `importDefer/importDeferNamespace.ts` | multi-file or JavaScript |  |
| `importDefer/importDeferTypeConflict1.ts` | multi-file or JavaScript |  |
| `importDefer/importDeferTypeConflict2.ts` | multi-file or JavaScript |  |
| `importDefer/importEqualsBindingDefer.ts` | multi-file or JavaScript |  |
| `importDefer/importMetaPropertyInvalidInCall.ts` | multi-file or JavaScript |  |
| `importDefer/typeofImportDefer.ts` | multi-file or JavaScript |  |
| `inferFromBindingPattern.ts` | not supported | expected `,` or `>`, on ` function f1<T extends string>(): T { return null as unknown as (T); } ` |
| `interfaces/declarationMerging/genericAndNonGenericInterfaceWithTheSameName.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `interfaces/declarationMerging/genericAndNonGenericInterfaceWithTheSameName2.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `interfaces/declarationMerging/mergedInheritedMembersSatisfyAbstractBase.ts` | not supported | expected `;` after expression, on ` abstract class BaseClass { ` |
| `interfaces/declarationMerging/mergedInterfacesWithConflictingPropertyNames.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `interfaces/declarationMerging/mergedInterfacesWithConflictingPropertyNames2.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `interfaces/declarationMerging/mergedInterfacesWithIndexers.ts` | not supported | index signatures |
| `interfaces/declarationMerging/mergedInterfacesWithIndexers2.ts` | not supported | index signatures |
| `interfaces/declarationMerging/mergedInterfacesWithInheritedPrivates.ts` | not supported | expected `:` and a type for the class field, on ` private x!: number; ` |
| `interfaces/declarationMerging/mergedInterfacesWithInheritedPrivates2.ts` | not supported | expected `:` and a type for the class field, on ` private x!: number; ` |
| `interfaces/declarationMerging/mergedInterfacesWithInheritedPrivates3.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `interfaces/declarationMerging/mergedInterfacesWithMultipleBases.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `interfaces/declarationMerging/mergedInterfacesWithMultipleBases2.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `interfaces/declarationMerging/mergedInterfacesWithMultipleBases3.ts` | not supported | duplicate declaration of interface `A`, on ` interface A<T> extends C<string>, C4<string> { ` |
| `interfaces/declarationMerging/mergedInterfacesWithMultipleBases4.ts` | not supported | duplicate declaration of interface `A`, on ` interface A<T> extends C<number>, C4<string> { ` |
| `interfaces/declarationMerging/mergeThreeInterfaces.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `interfaces/declarationMerging/mergeThreeInterfaces2.ts` | not supported | expected `;` after expression, on ` namespace M2 { ` |
| `interfaces/declarationMerging/mergeTwoInterfaces.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `interfaces/declarationMerging/mergeTwoInterfaces2.ts` | not supported | expected `;` after expression, on ` namespace M2 { ` |
| `interfaces/declarationMerging/twoGenericInterfacesDifferingByTypeParameterName.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `interfaces/declarationMerging/twoGenericInterfacesDifferingByTypeParameterName2.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `interfaces/declarationMerging/twoGenericInterfacesWithDifferentConstraints.ts` | not supported | expected `,` or `>`, on ` interface A<T extends Date> { ` |
| `interfaces/declarationMerging/twoGenericInterfacesWithTheSameNameButDifferentArity.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `interfaces/declarationMerging/twoInterfacesDifferentRootModule.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `interfaces/declarationMerging/twoInterfacesDifferentRootModule2.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `interfaces/declarationMerging/twoMergedInterfacesWithDifferingOverloads.ts` | not supported | duplicate declaration of interface `A`, on ` interface A { ` |
| `interfaces/declarationMerging/twoMergedInterfacesWithDifferingOverloads2.ts` | not supported | expected `;` after expression, on ` namespace G { ` |
| `interfaces/interfaceDeclarations/asiPreventsParsingAsInterface01.ts` | not supported | `interface` is a reserved keyword and can't be used as a name, on ` let interface: number = null as unknown as (number), I: string = null as unkn... ` |
| `interfaces/interfaceDeclarations/asiPreventsParsingAsInterface02.ts` | not supported | `interface` is a reserved keyword and can't be used as a name, on ` function f(interface: number, I: string): void { ` |
| `interfaces/interfaceDeclarations/asiPreventsParsingAsInterface03.ts` | not supported | `interface` is a reserved keyword and can't be used as a name, on ` let interface: number = null as unknown as (number), I: string = null as unkn... ` |
| `interfaces/interfaceDeclarations/asiPreventsParsingAsInterface04.ts` | not supported | expected `;` after declaration, on ` let declare: boolean = null as unknown as (boolean), interface: number = null... ` |
| `interfaces/interfaceDeclarations/asiPreventsParsingAsInterface05.ts` | not supported | `interface` is a reserved keyword and can't be used as a name, on ` let interface: number = 123; ` |
| `interfaces/interfaceDeclarations/derivedInterfaceDoesNotHideBaseSignatures.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `interfaces/interfaceDeclarations/derivedInterfaceIncompatibleWithBaseIndexer.ts` | not supported | index signatures |
| `interfaces/interfaceDeclarations/interfaceExtendingOptionalChain.ts` | not supported | expected `;` after expression, on ` namespace Foo { ` |
| `interfaces/interfaceDeclarations/interfaceExtendsObjectIntersection.ts` | not supported | intersection types |
| `interfaces/interfaceDeclarations/interfaceExtendsObjectIntersectionErrors.ts` | not supported | intersection types |
| `interfaces/interfaceDeclarations/interfaceThatHidesBaseProperty.ts` | checks too little | 0 after the port |
| `interfaces/interfaceDeclarations/interfaceThatHidesBaseProperty2.ts` | checks too little | 1 after the port |
| `interfaces/interfaceDeclarations/interfaceThatIndirectlyInheritsFromItself.ts` | not supported | expected `;` after expression, on ` namespace Generic { ` |
| `interfaces/interfaceDeclarations/interfaceThatInheritsFromItself.ts` | not supported | expected `{` after interface name, on ` interface Bar implements Bar { // error ` |
| `interfaces/interfaceDeclarations/interfaceWithAccessibilityModifiers.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` public a: any; ` |
| `interfaces/interfaceDeclarations/interfaceWithCallAndConstructSignature.ts` | not supported | construct signatures |
| `interfaces/interfaceDeclarations/interfaceWithCallSignaturesThatHidesBaseSignature.ts` | not supported | `as` to `Derived` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let d: Derived = null as unknown as (Derived); ` |
| `interfaces/interfaceDeclarations/interfaceWithCallSignaturesThatHidesBaseSignature2.ts` | not supported | `as` to `Derived` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let d: Derived = null as unknown as (Derived); ` |
| `interfaces/interfaceDeclarations/interfaceWithConstructSignaturesThatHidesBaseSignature.ts` | not supported | construct signatures |
| `interfaces/interfaceDeclarations/interfaceWithConstructSignaturesThatHidesBaseSignature2.ts` | not supported | construct signatures |
| `interfaces/interfaceDeclarations/interfaceWithMultipleBaseTypes.ts` | not supported | expected `;` after expression, on ` namespace Generic { ` |
| `interfaces/interfaceDeclarations/interfaceWithMultipleBaseTypes2.ts` | checks too little | 1 after the port |
| `interfaces/interfaceDeclarations/interfaceWithOverloadedCallAndConstructSignatures.ts` | not supported | construct signatures |
| `interfaces/interfaceDeclarations/interfaceWithPropertyOfEveryType.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `interfaces/interfaceDeclarations/interfaceWithPropertyThatIsPrivateInBaseType.ts` | checks too little | 2 after the port |
| `interfaces/interfaceDeclarations/interfaceWithPropertyThatIsPrivateInBaseType2.ts` | not supported | `any` is not supported, on ` x(): any; ` |
| `interfaces/interfaceDeclarations/interfaceWithSpecializedCallAndConstructSignatures.ts` | not supported | construct signatures |
| `interfaces/interfaceDeclarations/interfaceWithStringIndexerHidingBaseTypeIndexer2.ts` | not supported | index signatures |
| `interfaces/interfaceDeclarations/interfaceWithStringIndexerHidingBaseTypeIndexer3.ts` | not supported | index signatures |
| `interfaces/interfacesExtendingClasses/implementingAnInterfaceExtendingClassWithPrivates.ts` | checks too little | 4 after the port |
| `interfaces/interfacesExtendingClasses/implementingAnInterfaceExtendingClassWithPrivates2.ts` | not supported | expected `:` and a type for the class field, on ` private x!: string; ` |
| `interfaces/interfacesExtendingClasses/implementingAnInterfaceExtendingClassWithProtecteds.ts` | not supported | `protected` is not supported, on ` protected x: string; ` |
| `interfaces/interfacesExtendingClasses/interfaceExtendingClass.ts` | not supported | expected class member name, on ` [x: string]: Object; ` |
| `interfaces/interfacesExtendingClasses/interfaceExtendingClass2.ts` | not supported | expected class member name, on ` [x: string]: Object; ` |
| `interfaces/interfacesExtendingClasses/interfaceExtendingClassWithPrivates.ts` | not supported | expected `:` and a type for the class field, on ` private x!: string; ` |
| `interfaces/interfacesExtendingClasses/interfaceExtendingClassWithPrivates2.ts` | not supported | expected `:` and a type for the class field, on ` private x!: string; ` |
| `interfaces/interfacesExtendingClasses/interfaceExtendingClassWithProtecteds.ts` | not supported | `protected` is not supported, on ` protected x!: string; ` |
| `interfaces/interfacesExtendingClasses/interfaceExtendingClassWithProtecteds2.ts` | not supported | `protected` is not supported, on ` protected x!: string; ` |
| `internalModules/` | not supported | user-declared namespaces |
| `jsdoc/assertionsAndNonReturningFunctions.ts` | multi-file or JavaScript |  |
| `jsdoc/callbackCrossModule.ts` | multi-file or JavaScript |  |
| `jsdoc/callbackOnConstructor.ts` | multi-file or JavaScript |  |
| `jsdoc/callbackTag1.ts` | multi-file or JavaScript |  |
| `jsdoc/callbackTag2.ts` | multi-file or JavaScript |  |
| `jsdoc/callbackTag3.ts` | multi-file or JavaScript |  |
| `jsdoc/callbackTag4.ts` | multi-file or JavaScript |  |
| `jsdoc/callbackTagNamespace.ts` | multi-file or JavaScript |  |
| `jsdoc/callbackTagNestedParameter.ts` | multi-file or JavaScript |  |
| `jsdoc/callbackTagVariadicType.ts` | multi-file or JavaScript |  |
| `jsdoc/callOfPropertylessConstructorFunction.ts` | multi-file or JavaScript |  |
| `jsdoc/checkExportsObjectAssignProperty.ts` | multi-file or JavaScript |  |
| `jsdoc/checkExportsObjectAssignPrototypeProperty.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocOnEndOfFile.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocOptionalParamOrder.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocParamOnVariableDeclaredFunctionExpression.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocParamTag1.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocReturnTag1.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocReturnTag2.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocSatisfiesTag1.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocSatisfiesTag10.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocSatisfiesTag11.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocSatisfiesTag12.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocSatisfiesTag13.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocSatisfiesTag14.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocSatisfiesTag15.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocSatisfiesTag2.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocSatisfiesTag3.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocSatisfiesTag4.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocSatisfiesTag5.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocSatisfiesTag6.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocSatisfiesTag7.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocSatisfiesTag8.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocSatisfiesTag9.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocTypedefInParamTag1.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocTypedefOnlySourceFile.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocTypeTag1.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocTypeTag2.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocTypeTag3.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocTypeTag4.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocTypeTag5.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocTypeTag6.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocTypeTag7.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocTypeTag8.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocTypeTagOnObjectProperty1.ts` | multi-file or JavaScript |  |
| `jsdoc/checkJsdocTypeTagOnObjectProperty2.ts` | multi-file or JavaScript |  |
| `jsdoc/checkObjectDefineProperty.ts` | multi-file or JavaScript |  |
| `jsdoc/checkOtherObjectAssignProperty.ts` | multi-file or JavaScript |  |
| `jsdoc/constructorTagOnClassConstructor.ts` | multi-file or JavaScript |  |
| `jsdoc/constructorTagOnNestedBinaryExpression.ts` | multi-file or JavaScript |  |
| `jsdoc/constructorTagOnObjectLiteralMethod.ts` | multi-file or JavaScript |  |
| `jsdoc/constructorTagWithThisTag.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsClassAccessor.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsClasses.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsClassesErr.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsClassExtendsVisibility.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsClassImplementsGenericsSerialization.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsClassLeadingOptional.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsClassLikeHeuristic.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsClassMethod.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsClassStatic.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsClassStatic2.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsClassStaticMethodAugmentation.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsCommonjsRelativePath.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsComputedNames.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsConstsAsNamespacesWithReferences.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsCrossfileMerge.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsDefault.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsDefaultsErr.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsDocCommentsOnConsts.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsEnums.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsEnumTag.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportAssignedClassExpression.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportAssignedClassExpressionAnonymous.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportAssignedClassExpressionAnonymousWithSub.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportAssignedClassExpressionShadowing.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportAssignedClassInstance1.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportAssignedClassInstance2.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportAssignedClassInstance3.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportAssignedConstructorFunction.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportAssignedConstructorFunctionWithSub.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportAssignedVisibility.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportAssignmentExpressionPlusSecondary.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportAssignmentWithKeywordName.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportDefinePropertyEmit.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportDoubleAssignmentInClosure.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportedClassAliases.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportForms.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportFormsErr.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportSpecifierNonlocal.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsExportSubAssignments.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsFunctionClassesCjsExportAssignment.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsFunctionJSDoc.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsFunctionKeywordProp.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsFunctionKeywordPropExhaustive.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsFunctionLikeClasses.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsFunctionLikeClasses2.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsFunctionPrototypeStatic.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsFunctions.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsFunctionsCjs.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsFunctionWithDefaultAssignedMember.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsGetterSetter.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsImportAliasExposedWithinNamespace.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsImportAliasExposedWithinNamespaceCjs.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsImportNamespacedType.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsImportTypeBundled.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsInterfaces.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsJSDocRedirectedLookups.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsJson.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsMissingGenerics.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsMissingTypeParameters.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsModuleReferenceHasEmit.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsMultipleExportFromMerge.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsNestedParams.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsNonIdentifierInferredNames.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsOptionalTypeLiteralProps1.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsOptionalTypeLiteralProps2.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsPackageJson.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsParameterTagReusesInputNodeInEmit1.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsParameterTagReusesInputNodeInEmit2.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsPrivateFields01.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsReactComponents.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsReexportAliases.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsReexportAliasesEsModuleInterop.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsReexportedCjsAlias.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsReferenceToClassInstanceCrossFile.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsRestArgsWithThisTypeInJSDocFunction.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsReusesExistingNodesMappingJSDocTypes.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsReusesExistingTypeAnnotations.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsSubclassWithExplicitNoArgumentConstructor.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsThisTypes.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsTypeAliases.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsTypedefAndImportTypes.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsTypedefAndLatebound.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsTypedefDescriptionsPreserved.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsTypedefFunction.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsTypedefPropertyAndExportAssignment.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsTypeReassignmentFromDeclaration.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsTypeReassignmentFromDeclaration2.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsTypeReferences.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsTypeReferences2.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsTypeReferences3.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsTypeReferences4.ts` | multi-file or JavaScript |  |
| `jsdoc/declarations/jsDeclarationsUniqueSymbolUsage.ts` | multi-file or JavaScript |  |
| `jsdoc/enumTag.ts` | multi-file or JavaScript |  |
| `jsdoc/enumTagCircularReference.ts` | multi-file or JavaScript |  |
| `jsdoc/enumTagImported.ts` | multi-file or JavaScript |  |
| `jsdoc/enumTagOnExports.ts` | multi-file or JavaScript |  |
| `jsdoc/enumTagOnExports2.ts` | multi-file or JavaScript |  |
| `jsdoc/enumTagUseBeforeDefCrash.ts` | multi-file or JavaScript |  |
| `jsdoc/errorIsolation.ts` | multi-file or JavaScript |  |
| `jsdoc/errorOnFunctionReturnType.ts` | multi-file or JavaScript |  |
| `jsdoc/exportedAliasedEnumTag.ts` | multi-file or JavaScript |  |
| `jsdoc/exportedEnumTypeAndValue.ts` | multi-file or JavaScript |  |
| `jsdoc/extendsTag1.ts` | multi-file or JavaScript |  |
| `jsdoc/extendsTag2.ts` | multi-file or JavaScript |  |
| `jsdoc/extendsTag3.ts` | multi-file or JavaScript |  |
| `jsdoc/extendsTag4.ts` | multi-file or JavaScript |  |
| `jsdoc/extendsTag5.ts` | multi-file or JavaScript |  |
| `jsdoc/extendsTag6.ts` | multi-file or JavaScript |  |
| `jsdoc/extendsTagEmit.ts` | multi-file or JavaScript |  |
| `jsdoc/importDeferJsdoc.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag1.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag10.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag11.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag12.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag13.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag14.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag15.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag16.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag17.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag18.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag19.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag2.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag20.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag21.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag22.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag23.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag24.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag25.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag3.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag4.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag5.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag6.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag7.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag8.ts` | multi-file or JavaScript |  |
| `jsdoc/importTag9.ts` | multi-file or JavaScript |  |
| `jsdoc/inferThis.ts` | multi-file or JavaScript |  |
| `jsdoc/instantiateTemplateTagTypeParameterOnVariableStatement.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocAccessibilityTags.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocAccessibilityTagsDeclarations.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocAugments_errorInExtendsExpression.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocAugments_nameMismatch.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocAugments_noExtends.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocAugments_notAClass.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocAugments_qualifiedName.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocAugments_withTypeParameter.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocAugmentsMissingType.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocBindingInUnreachableCode.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocCatchClauseWithTypeAnnotation.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocDisallowedInTypescript.ts` | not supported | expected identifier after `.` in type name, on ` let ara: Array.<number> = [1,2,3]; ` |
| `jsdoc/jsdocFunction_missingReturn.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocFunctionType.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocImplements_class.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocImplements_interface_multiple.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocImplements_interface.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocImplements_missingType.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocImplements_namespacedInterface.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocImplements_properties.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocImplements_signatures.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocImplementsTag.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocImportType.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocImportType2.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocImportTypeReferenceToClassAlias.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocImportTypeReferenceToCommonjsModule.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocImportTypeReferenceToESModule.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocImportTypeReferenceToStringLiteral.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocIndexSignature.ts` | not supported | index signatures |
| `jsdoc/jsdocLinkTag1.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocLinkTag2.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocLinkTag3.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocLinkTag4.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocLinkTag5.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocLinkTag6.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocLinkTag7.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocLinkTag8.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocLinkTag9.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocLiteral.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocNeverUndefinedNull.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocOuterTypeParameters1.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocOuterTypeParameters2.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocOuterTypeParameters3.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocOverrideTag1.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocParamTag2.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocParamTagTypeLiteral.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocParseBackquotedParamName.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocParseDotDotDotInJSDocFunction.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocParseErrorsInTypescript.ts` | not supported | expected expression, on ` function parse1(n: number=): void { } ` |
| `jsdoc/jsdocParseHigherOrderFunction.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocParseMatchingBackticks.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocParseParenthesizedJSDocParameter.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocParseStarEquals.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocPostfixEqualsAddsOptionality.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocPrefixPostfixParsing.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocPrivateName1.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocPrivateName2.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocPrototypePropertyAccessWithType.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocReadonly.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocReadonlyDeclarations.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocReturnTag1.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocSignatureOnReturnedFunction.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTemplateClass.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTemplateConstructorFunction.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTemplateConstructorFunction2.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTemplateTag.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTemplateTag2.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTemplateTag3.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTemplateTag4.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTemplateTag5.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTemplateTag6.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTemplateTag7.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTemplateTag8.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTemplateTagDefault.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTemplateTagNameResolution.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocThisType.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTwoLineTypedef.ts` | checks too little | 0 after the port |
| `jsdoc/jsdocTypeDefAtStartOfFile.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTypeFromChainedAssignment.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTypeFromChainedAssignment2.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTypeFromChainedAssignment3.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTypeReferenceExports.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTypeReferenceToImport.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTypeReferenceToImportOfClassExpression.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTypeReferenceToImportOfFunctionExpression.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTypeReferenceToMergedClass.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTypeReferenceToValue.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTypeReferenceUseBeforeDef.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTypeTag.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTypeTagCast.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTypeTagOnParameter1.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTypeTagParameterType.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocTypeTagRequiredParameters.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocVariableDeclarationWithTypeAnnotation.ts` | multi-file or JavaScript |  |
| `jsdoc/jsdocVariadicType.ts` | multi-file or JavaScript |  |
| `jsdoc/linkTagEmit1.ts` | multi-file or JavaScript |  |
| `jsdoc/moduleExportsElementAccessAssignment.ts` | multi-file or JavaScript |  |
| `jsdoc/moduleExportsElementAccessAssignment2.ts` | multi-file or JavaScript |  |
| `jsdoc/noAssertForUnparseableTypedefs.ts` | multi-file or JavaScript |  |
| `jsdoc/noDuplicateJsdoc1.ts` | multi-file or JavaScript |  |
| `jsdoc/overloadTag1.ts` | multi-file or JavaScript |  |
| `jsdoc/overloadTag2.ts` | multi-file or JavaScript |  |
| `jsdoc/overloadTag3.ts` | multi-file or JavaScript |  |
| `jsdoc/paramTagBracketsAddOptionalUndefined.ts` | multi-file or JavaScript |  |
| `jsdoc/paramTagNestedWithoutTopLevelObject.ts` | multi-file or JavaScript |  |
| `jsdoc/paramTagNestedWithoutTopLevelObject2.ts` | multi-file or JavaScript |  |
| `jsdoc/paramTagNestedWithoutTopLevelObject3.ts` | multi-file or JavaScript |  |
| `jsdoc/paramTagNestedWithoutTopLevelObject4.ts` | multi-file or JavaScript |  |
| `jsdoc/paramTagOnCallExpression.ts` | multi-file or JavaScript |  |
| `jsdoc/paramTagOnFunctionUsingArguments.ts` | multi-file or JavaScript |  |
| `jsdoc/paramTagTypeResolution.ts` | multi-file or JavaScript |  |
| `jsdoc/paramTagTypeResolution2.ts` | multi-file or JavaScript |  |
| `jsdoc/paramTagWrapping.ts` | multi-file or JavaScript |  |
| `jsdoc/parseLinkTag.ts` | the port changes what it checks | `tsc` then reports TS7034, TS7005 |
| `jsdoc/parseThrowsTag.ts` | checks too little | 0 after the port |
| `jsdoc/returnTagTypeGuard.ts` | multi-file or JavaScript |  |
| `jsdoc/seeTag1.ts` | not supported | expected `;` after expression, on ` namespace NS { ` |
| `jsdoc/seeTag3.ts` | multi-file or JavaScript |  |
| `jsdoc/seeTag4.ts` | multi-file or JavaScript |  |
| `jsdoc/syntaxErrors.ts` | multi-file or JavaScript |  |
| `jsdoc/templateInsideCallback.ts` | multi-file or JavaScript |  |
| `jsdoc/thisPrototypeMethodCompoundAssignment.ts` | not supported | expected expression, on ` Element.prototype.remove ??= function () { ` |
| `jsdoc/thisPrototypeMethodCompoundAssignmentJs.ts` | multi-file or JavaScript |  |
| `jsdoc/thisTag1.ts` | multi-file or JavaScript |  |
| `jsdoc/thisTag2.ts` | multi-file or JavaScript |  |
| `jsdoc/thisTag3.ts` | multi-file or JavaScript |  |
| `jsdoc/tsNoCheckForTypescript.ts` | multi-file or JavaScript |  |
| `jsdoc/tsNoCheckForTypescriptComments1.ts` | multi-file or JavaScript |  |
| `jsdoc/tsNoCheckForTypescriptComments2.ts` | multi-file or JavaScript |  |
| `jsdoc/typedefCrossModule.ts` | multi-file or JavaScript |  |
| `jsdoc/typedefCrossModule2.ts` | multi-file or JavaScript |  |
| `jsdoc/typedefCrossModule3.ts` | multi-file or JavaScript |  |
| `jsdoc/typedefCrossModule4.ts` | multi-file or JavaScript |  |
| `jsdoc/typedefCrossModule5.ts` | multi-file or JavaScript |  |
| `jsdoc/typedefDuplicateTypeDeclaration.ts` | multi-file or JavaScript |  |
| `jsdoc/typedefInnerNamepaths.ts` | multi-file or JavaScript |  |
| `jsdoc/typedefMultipleTypeParameters.ts` | multi-file or JavaScript |  |
| `jsdoc/typedefOnSemicolonClassElement.ts` | multi-file or JavaScript |  |
| `jsdoc/typedefOnStatements.ts` | multi-file or JavaScript |  |
| `jsdoc/typedefScope1.ts` | multi-file or JavaScript |  |
| `jsdoc/typedefTagExtraneousProperty.ts` | multi-file or JavaScript |  |
| `jsdoc/typedefTagNested.ts` | multi-file or JavaScript |  |
| `jsdoc/typedefTagTypeResolution.ts` | multi-file or JavaScript |  |
| `jsdoc/typedefTagWrapping.ts` | multi-file or JavaScript |  |
| `jsdoc/typeParameterExtendsUnionConstraintDistributed.ts` | not supported | unexpected character `&`, on ` function f<T extends A>(a: T): A & T { return a; } // Shouldn't error ` |
| `jsdoc/typeTagCircularReferenceOnConstructorFunction.ts` | multi-file or JavaScript |  |
| `jsdoc/typeTagModuleExports.ts` | multi-file or JavaScript |  |
| `jsdoc/typeTagNoErasure.ts` | multi-file or JavaScript |  |
| `jsdoc/typeTagOnPropertyAssignment.ts` | multi-file or JavaScript |  |
| `jsdoc/typeTagPrototypeAssignment.ts` | multi-file or JavaScript |  |
| `jsdoc/typeTagWithGenericSignature.ts` | multi-file or JavaScript |  |
| `jsx/jsxAttributeInitializer.ts` | multi-file or JavaScript |  |
| `jsx/jsxUnclosedParserRecovery.ts` | multi-file or JavaScript |  |
| `jsx/tsxEmitSpreadAttribute.ts` | multi-file or JavaScript |  |
| `jsx/tsxReactEmitSpreadAttribute.ts` | multi-file or JavaScript |  |
| `moduleResolution/allowImportingTsExtensions.ts` | multi-file or JavaScript |  |
| `moduleResolution/allowImportingTypesDtsExtension.ts` | multi-file or JavaScript |  |
| `moduleResolution/bundler/bundlerCommonJS.ts` | multi-file or JavaScript |  |
| `moduleResolution/bundler/bundlerConditionsExcludesNode.ts` | multi-file or JavaScript |  |
| `moduleResolution/bundler/bundlerDirectoryModule.ts` | multi-file or JavaScript |  |
| `moduleResolution/bundler/bundlerImportESM.ts` | multi-file or JavaScript |  |
| `moduleResolution/bundler/bundlerImportTsExtensions.ts` | multi-file or JavaScript |  |
| `moduleResolution/bundler/bundlerNodeModules1.ts` | multi-file or JavaScript |  |
| `moduleResolution/bundler/bundlerOptionsCompat.ts` | multi-file or JavaScript |  |
| `moduleResolution/bundler/bundlerRelative1.ts` | multi-file or JavaScript |  |
| `moduleResolution/bundler/bundlerSyntaxRestrictions.ts` | multi-file or JavaScript |  |
| `moduleResolution/conditionalExportsResolutionFallback.ts` | multi-file or JavaScript |  |
| `moduleResolution/conditionalExportsResolutionFallbackNull.ts` | multi-file or JavaScript |  |
| `moduleResolution/customConditions.ts` | multi-file or JavaScript |  |
| `moduleResolution/declarationNotFoundPackageBundlesTypes.ts` | multi-file or JavaScript |  |
| `moduleResolution/extensionLoadingPriority.ts` | multi-file or JavaScript |  |
| `moduleResolution/importFromDot.ts` | multi-file or JavaScript |  |
| `moduleResolution/nestedPackageJsonRedirect.ts` | multi-file or JavaScript |  |
| `moduleResolution/node10AlternateResult_noResolution.ts` | multi-file or JavaScript |  |
| `moduleResolution/node10Alternateresult_noTypes.ts` | multi-file or JavaScript |  |
| `moduleResolution/node10IsNode_node.ts` | multi-file or JavaScript |  |
| `moduleResolution/node10IsNode_node10.ts` | multi-file or JavaScript |  |
| `moduleResolution/nodeModulesAtTypesPriority.ts` | multi-file or JavaScript |  |
| `moduleResolution/packageJsonExportsOptionsCompat.ts` | multi-file or JavaScript |  |
| `moduleResolution/packageJsonImportsExportsOptionCompat.ts` | multi-file or JavaScript |  |
| `moduleResolution/packageJsonMain_isNonRecursive.ts` | multi-file or JavaScript |  |
| `moduleResolution/packageJsonMain.ts` | multi-file or JavaScript |  |
| `moduleResolution/resolutionModeCache.ts` | multi-file or JavaScript |  |
| `moduleResolution/resolutionModeImportType1.ts` | multi-file or JavaScript |  |
| `moduleResolution/resolutionModeTripleSlash1.ts` | multi-file or JavaScript |  |
| `moduleResolution/resolutionModeTripleSlash2.ts` | multi-file or JavaScript |  |
| `moduleResolution/resolutionModeTripleSlash3.ts` | multi-file or JavaScript |  |
| `moduleResolution/resolutionModeTripleSlash4.ts` | multi-file or JavaScript |  |
| `moduleResolution/resolutionModeTripleSlash5.ts` | multi-file or JavaScript |  |
| `moduleResolution/resolutionModeTypeOnlyImport1.ts` | multi-file or JavaScript |  |
| `moduleResolution/resolvesWithoutExportsDiagnostic1.ts` | multi-file or JavaScript |  |
| `moduleResolution/scopedPackages.ts` | multi-file or JavaScript |  |
| `moduleResolution/scopedPackagesClassic.ts` | multi-file or JavaScript |  |
| `moduleResolution/selfNameModuleAugmentation.ts` | multi-file or JavaScript |  |
| `moduleResolution/typesVersions.ambientModules.ts` | multi-file or JavaScript |  |
| `moduleResolution/typesVersions.emptyTypes.ts` | multi-file or JavaScript |  |
| `moduleResolution/typesVersions.justIndex.ts` | multi-file or JavaScript |  |
| `moduleResolution/typesVersions.multiFile.ts` | multi-file or JavaScript |  |
| `moduleResolution/untypedModuleImport_allowJs.ts` | multi-file or JavaScript |  |
| `moduleResolution/untypedModuleImport_noImplicitAny_relativePath.ts` | multi-file or JavaScript |  |
| `moduleResolution/untypedModuleImport_noImplicitAny_scoped.ts` | multi-file or JavaScript |  |
| `moduleResolution/untypedModuleImport_noImplicitAny_typesForPackageExist.ts` | multi-file or JavaScript |  |
| `moduleResolution/untypedModuleImport_noImplicitAny.ts` | multi-file or JavaScript |  |
| `moduleResolution/untypedModuleImport_vsAmbient.ts` | multi-file or JavaScript |  |
| `moduleResolution/untypedModuleImport_withAugmentation.ts` | multi-file or JavaScript |  |
| `moduleResolution/untypedModuleImport.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeAllowJsPackageSelfName.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeAllowJsPackageSelfName2.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJs1.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsCjsFromJs.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsConditionalPackageExports.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsDynamicImport.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsExportAssignment.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsExportlessJsModuleDetectionAuto.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsGeneratedNameCollisions.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsImportAssignment.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsImportHelpersCollisions1.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsImportHelpersCollisions2.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsImportHelpersCollisions3.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsImportMeta.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsPackageExports.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsPackageImports.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsPackagePatternExports.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsPackagePatternExportsExclude.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsPackagePatternExportsTrailers.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsSynchronousCallErrors.ts` | multi-file or JavaScript |  |
| `node/allowJs/nodeModulesAllowJsTopLevelAwait.ts` | multi-file or JavaScript |  |
| `node/esmModuleExports1.ts` | multi-file or JavaScript |  |
| `node/esmModuleExports2.ts` | multi-file or JavaScript |  |
| `node/esmModuleExports3.ts` | multi-file or JavaScript |  |
| `node/legacyNodeModulesExportsSpecifierGenerationConditions.ts` | multi-file or JavaScript |  |
| `node/nodeModules1.ts` | multi-file or JavaScript |  |
| `node/nodeModulesCJSEmit1.ts` | multi-file or JavaScript |  |
| `node/nodeModulesCjsFormatFileAlwaysHasDefault.ts` | multi-file or JavaScript |  |
| `node/nodeModulesCJSResolvingToESM1_emptyPackageJson.ts` | multi-file or JavaScript |  |
| `node/nodeModulesCJSResolvingToESM2_cjsPackageJson.ts` | multi-file or JavaScript |  |
| `node/nodeModulesCJSResolvingToESM3_modulePackageJson.ts` | multi-file or JavaScript |  |
| `node/nodeModulesCJSResolvingToESM4_noPackageJson.ts` | multi-file or JavaScript |  |
| `node/nodeModulesConditionalPackageExports.ts` | multi-file or JavaScript |  |
| `node/nodeModulesDeclarationEmitDynamicImportWithPackageExports.ts` | multi-file or JavaScript |  |
| `node/nodeModulesDeclarationEmitWithPackageExports.ts` | multi-file or JavaScript |  |
| `node/nodeModulesDynamicImport.ts` | multi-file or JavaScript |  |
| `node/nodeModulesExportAssignments.ts` | multi-file or JavaScript |  |
| `node/nodeModulesExportsBlocksSpecifierResolution.ts` | multi-file or JavaScript |  |
| `node/nodeModulesExportsBlocksTypesVersions.ts` | multi-file or JavaScript |  |
| `node/nodeModulesExportsDoubleAsterisk.ts` | multi-file or JavaScript |  |
| `node/nodeModulesExportsSourceTs.ts` | multi-file or JavaScript |  |
| `node/nodeModulesExportsSpecifierGenerationConditions.ts` | multi-file or JavaScript |  |
| `node/nodeModulesExportsSpecifierGenerationDirectory.ts` | multi-file or JavaScript |  |
| `node/nodeModulesExportsSpecifierGenerationPattern.ts` | multi-file or JavaScript |  |
| `node/nodeModulesForbidenSyntax.ts` | multi-file or JavaScript |  |
| `node/nodeModulesGeneratedNameCollisions.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportAssertions.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportAssignments.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportAttributes.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportAttributesModeDeclarationEmit1.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportAttributesModeDeclarationEmit2.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportAttributesModeDeclarationEmitErrors.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportAttributesTypeModeDeclarationEmit.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportAttributesTypeModeDeclarationEmitErrors.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportHelpersCollisions.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportHelpersCollisions2.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportHelpersCollisions3.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportMeta.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportModeDeclarationEmit1.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportModeDeclarationEmit2.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportModeDeclarationEmitErrors1.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportResolutionIntoExport.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportResolutionNoCycle.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportTypeModeDeclarationEmit1.ts` | multi-file or JavaScript |  |
| `node/nodeModulesImportTypeModeDeclarationEmitErrors1.ts` | multi-file or JavaScript |  |
| `node/nodeModulesJson.ts` | multi-file or JavaScript |  |
| `node/nodeModulesNoDirectoryModule.ts` | multi-file or JavaScript |  |
| `node/nodeModulesPackageExports.ts` | multi-file or JavaScript |  |
| `node/nodeModulesPackageImports.ts` | multi-file or JavaScript |  |
| `node/nodeModulesPackageImportsRootWildcard.ts` | multi-file or JavaScript |  |
| `node/nodeModulesPackageImportsRootWildcardNode16.ts` | multi-file or JavaScript |  |
| `node/nodeModulesPackagePatternExports.ts` | multi-file or JavaScript |  |
| `node/nodeModulesPackagePatternExportsExclude.ts` | multi-file or JavaScript |  |
| `node/nodeModulesPackagePatternExportsTrailers.ts` | multi-file or JavaScript |  |
| `node/nodeModulesResolveJsonModule.ts` | multi-file or JavaScript |  |
| `node/nodeModulesSynchronousCallErrors.ts` | multi-file or JavaScript |  |
| `node/nodeModulesTopLevelAwait.ts` | multi-file or JavaScript |  |
| `node/nodeModulesTripleSlashReferenceModeDeclarationEmit1.ts` | multi-file or JavaScript |  |
| `node/nodeModulesTripleSlashReferenceModeDeclarationEmit2.ts` | multi-file or JavaScript |  |
| `node/nodeModulesTripleSlashReferenceModeDeclarationEmit3.ts` | multi-file or JavaScript |  |
| `node/nodeModulesTripleSlashReferenceModeDeclarationEmit4.ts` | multi-file or JavaScript |  |
| `node/nodeModulesTripleSlashReferenceModeDeclarationEmit5.ts` | multi-file or JavaScript |  |
| `node/nodeModulesTripleSlashReferenceModeDeclarationEmit6.ts` | multi-file or JavaScript |  |
| `node/nodeModulesTripleSlashReferenceModeDeclarationEmit7.ts` | multi-file or JavaScript |  |
| `node/nodeModulesTripleSlashReferenceModeOverride1.ts` | multi-file or JavaScript |  |
| `node/nodeModulesTripleSlashReferenceModeOverride2.ts` | multi-file or JavaScript |  |
| `node/nodeModulesTripleSlashReferenceModeOverride3.ts` | multi-file or JavaScript |  |
| `node/nodeModulesTripleSlashReferenceModeOverride4.ts` | multi-file or JavaScript |  |
| `node/nodeModulesTripleSlashReferenceModeOverride5.ts` | multi-file or JavaScript |  |
| `node/nodeModulesTripleSlashReferenceModeOverrideModeError.ts` | multi-file or JavaScript |  |
| `node/nodeModulesTripleSlashReferenceModeOverrideOldResolutionError.ts` | multi-file or JavaScript |  |
| `node/nodeModulesTypesVersionPackageExports.ts` | multi-file or JavaScript |  |
| `node/nodePackageSelfName.ts` | multi-file or JavaScript |  |
| `node/nodePackageSelfNameScoped.ts` | multi-file or JavaScript |  |
| `nonjsExtensions/declarationFileForHtmlFileWithinDeclarationFile.ts` | multi-file or JavaScript |  |
| `nonjsExtensions/declarationFileForHtmlImport.ts` | multi-file or JavaScript |  |
| `nonjsExtensions/declarationFileForJsonImport.ts` | multi-file or JavaScript |  |
| `nonjsExtensions/declarationFileForTsJsImport.ts` | multi-file or JavaScript |  |
| `nonjsExtensions/declarationFilesForNodeNativeModules.ts` | multi-file or JavaScript |  |
| `override/override_js1.ts` | multi-file or JavaScript |  |
| `override/override_js2.ts` | multi-file or JavaScript |  |
| `override/override_js3.ts` | multi-file or JavaScript |  |
| `override/override_js4.ts` | multi-file or JavaScript |  |
| `override/override1.ts` | the port changes what it checks | `tsc` then reports TS1003, TS1005, TS2304 |
| `override/override10.ts` | not supported | expected `;` after expression, on ` abstract class Base { ` |
| `override/override11.ts` | not supported | parameter requires a type annotation, on ` constructor (override public foo: number) { ` |
| `override/override12.ts` | not supported | expected `:` and a type for the class field, on ` override m1(): number { ` |
| `override/override14.ts` | not supported | expected `:` and a type for the class field, on ` declare property: number ` |
| `override/override15.ts` | not supported | expected `:` and a type for the class field, on ` override doSomethang(): void {} ` |
| `override/override16.ts` | not supported | expected `:` and a type for the class field, on ` override foo: string = "string"; ` |
| `override/override17.ts` | duplicate | of `override/override12.ts` |
| `override/override18.ts` | not supported | expected `:` and a type for the class field, on ` override foo: string = "string"; ` |
| `override/override19.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `override/override2.ts` | not supported | expected `;` after expression, on ` abstract class AB { ` |
| `override/override20.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `override/override21.ts` | not supported | expected `:` and a type for the class field, on ` override [foo](): void { } ` |
| `override/override3.ts` | not supported | expected `;` after expression, on ` declare class B { ` |
| `override/override4.ts` | not supported | expected `:` and a type for the class field, on ` override p2: number = 3; ` |
| `override/override5.ts` | not supported | expected `:` and a type for the class field, on ` declare p1: number ` |
| `override/override6.ts` | checks too little | 2 after the port |
| `override/override7.ts` | not supported | expected `:` and a type for the class field, on ` declare p1: number ` |
| `override/override9.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` override bar(): void; ` |
| `override/overrideDynamicName1.ts` | not supported | expected class member name, on ` [prop](): void {} ` |
| `override/overrideKeywordOrder.ts` | not supported | expected `;` after expression, on ` abstract class Base { ` |
| `override/overrideLateBindableIndexSignature1.ts` | not supported | index signatures |
| `override/overrideLateBindableName1.ts` | not supported | expected class member name, on ` [prop](): void {} ` |
| `override/overrideParameterProperty.ts` | not supported | expected `:` and a type for the class field, on ` p1!: string; ` |
| `override/overrideWithoutNoImplicitOverride1.ts` | not supported | expected a declaration after `export`, on ` export declare class AmbientClass { ` |
| `parser/ecmascript2018/asyncGenerators/` | not supported | async/await and generators |
| `parser/ecmascript2018/forAwait/` | not supported | async/await |
| `parser/ecmascript2021/numericSeparators/parser.numericSeparators.binary.ts` | not supported | expected `;` after expression, on ` 0b00_11; ` |
| `parser/ecmascript2021/numericSeparators/parser.numericSeparators.binaryNegative.ts` | multi-file or JavaScript |  |
| `parser/ecmascript2021/numericSeparators/parser.numericSeparators.decimal.ts` | not supported | expected `;` after expression, on ` 1_000_000_000 ` |
| `parser/ecmascript2021/numericSeparators/parser.numericSeparators.decmialNegative.ts` | multi-file or JavaScript |  |
| `parser/ecmascript2021/numericSeparators/parser.numericSeparators.hex.ts` | not supported | expected `;` after expression, on ` 0x00_11; ` |
| `parser/ecmascript2021/numericSeparators/parser.numericSeparators.hexNegative.ts` | multi-file or JavaScript |  |
| `parser/ecmascript2021/numericSeparators/parser.numericSeparators.octal.ts` | not supported | expected `;` after expression, on ` 0o00_11; ` |
| `parser/ecmascript2021/numericSeparators/parser.numericSeparators.octalNegative.ts` | multi-file or JavaScript |  |
| `parser/ecmascript2021/numericSeparators/parser.numericSeparators.unicodeEscape.ts` | multi-file or JavaScript |  |
| `parser/ecmascript3/Accessors/parserES3Accessors1.ts` | checks too little | 1 after the port |
| `parser/ecmascript3/Accessors/parserES3Accessors2.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `parser/ecmascript3/Accessors/parserES3Accessors3.ts` | not supported | expected `,` or `}`, on ` let v = { get Foo() { } }; ` |
| `parser/ecmascript3/Accessors/parserES3Accessors4.ts` | not supported | expected `,` or `}`, on ` let v = { set Foo(a) { } }; ` |
| `parser/ecmascript5/Accessors/parserAccessors1.ts` | duplicate | of `parser/ecmascript3/Accessors/parserES3Accessors1.ts` |
| `parser/ecmascript5/Accessors/parserAccessors10.ts` | not supported | expected `,` or `}`, on ` public get foo() { } ` |
| `parser/ecmascript5/Accessors/parserAccessors2.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `parser/ecmascript5/Accessors/parserAccessors3.ts` | not supported | expected `,` or `}`, on ` let v = { get Foo() { } }; ` |
| `parser/ecmascript5/Accessors/parserAccessors4.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `parser/ecmascript5/Accessors/parserAccessors5.ts` | not supported | expected `;` after expression, on ` declare class C { ` |
| `parser/ecmascript5/Accessors/parserAccessors6.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `parser/ecmascript5/Accessors/parserAccessors7.ts` | not supported | expected `,` or `}`, on ` let v = { get foo(v: number) { } }; ` |
| `parser/ecmascript5/Accessors/parserAccessors8.ts` | the port changes what it checks | `tsc` then reports TS7032 |
| `parser/ecmascript5/Accessors/parserAccessors9.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `parser/ecmascript5/Accessors/parserGetAccessorWithTypeParameters1.ts` | not supported | expected `:` and a type for the class field, on ` get foo<T>() { } ` |
| `parser/ecmascript5/Accessors/parserSetAccessorWithTypeAnnotation1.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `parser/ecmascript5/Accessors/parserSetAccessorWithTypeParameters1.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression1.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression10.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression11.ts` | not supported | expected expression, on ` let v = [1,,1]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression12.ts` | not supported | expected expression, on ` let v = [1,,,1]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression13.ts` | not supported | expected expression, on ` let v = [1,,1,,1]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression14.ts` | not supported | expected expression, on ` let v = [,,1,1,,1,,1,1,,1]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression15.ts` | not supported | expected expression, on ` let v = [,,1,1,,1,,1,1,,1,]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression2.ts` | not supported | expected expression, on ` let v = [,]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression3.ts` | not supported | expected expression, on ` let v = [,,]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression4.ts` | not supported | expected expression, on ` let v = [,,,]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression5.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression6.ts` | not supported | expected expression, on ` let v = [,1]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression7.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression8.ts` | not supported | expected expression, on ` let v = [,1,]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression9.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression1.ts` | not supported | expected `,` or `)`, on ` let v = (public x: string) => { }; ` |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression10.ts` | multi-file or JavaScript |  |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression11.ts` | multi-file or JavaScript |  |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression12.ts` | multi-file or JavaScript |  |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression13.ts` | multi-file or JavaScript |  |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression14.ts` | multi-file or JavaScript |  |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression15.ts` | multi-file or JavaScript |  |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression16.ts` | multi-file or JavaScript |  |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression17.ts` | multi-file or JavaScript |  |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression2.ts` | not supported | expected `;` after assignment, on ` a = () => { } \|\| a ` |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression3.ts` | porter failure | nothing to prune at offsets 38, 38; our first unsupported error: expected `)` |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression4.ts` | not supported | expected `)`, on ` a = (() => { }, a) ` |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression5.ts` | checks too little | 3 after the port |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression6.ts` | not supported | expected `)`, on ` return true ? (q ? true : false) : (b = q.length, function() { }); ` |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression7.ts` | not supported | expected `,` or `}`, on ` async m() { ` |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression8.ts` | multi-file or JavaScript |  |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression9.ts` | multi-file or JavaScript |  |
| `parser/ecmascript5/AutomaticSemicolonInsertion/parserAutomaticSemicolonInsertion1.ts` | not supported | expected field name in object type, on ` (): void ` |
| `parser/ecmascript5/CatchClauses/parserCatchClauseWithTypeAnnotation1.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/ClassDeclarations/parserClass1.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration1.ts` | not supported | expected `{` after class header, on ` class C extends A extends B { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration10.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration11.ts` | not supported | expected `{`, on ` constructor(); ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration12.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration13.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration14.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration15.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration16.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration17.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration18.ts` | not supported | expected `;` after expression, on ` declare class FooBase { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration19.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration2.ts` | not supported | expected `{` after class header, on ` class C implements A implements B { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration20.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration21.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration22.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration23.ts` | not supported | unexpected character `\`, on ` class C\u0032 { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration24.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration25.ts` | not supported | expected `{`, on ` data(): U; ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration26.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration3.ts` | not supported | expected `{` after class header, on ` class C implements A extends B { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration4.ts` | not supported | expected `{` after class header, on ` class C extends A implements B extends C { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration5.ts` | not supported | expected `{` after class header, on ` class C extends A implements B implements C { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration6.ts` | not supported | expected `{` after class header, on ` class C extends A, B { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration7.ts` | not supported | expected `;` after expression, on ` declare namespace M { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration8.ts` | not supported | expected `{`, on ` constructor(); ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration9.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclarationIndexSignature1.ts` | not supported | index signatures |
| `parser/ecmascript5/ComputedPropertyNames/` | not supported | computed property names |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration1.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration10.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration11.ts` | not supported | expected expression, on ` } ` |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration2.ts` | not supported | a constructor cannot be `static`, on ` static constructor() { } ` |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration3.ts` | not supported | expected `:` and a type for the class field, on ` export constructor() { } ` |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration4.ts` | not supported | expected `:` and a type for the class field, on ` declare constructor() { } ` |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration5.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration6.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration7.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration8.ts` | not supported | expected `:` and a type for the class field, on ` public constructor; ` |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration9.ts` | not supported | expected `:` and return type, on ` constructor<T>() { } ` |
| `parser/ecmascript5/EnumDeclarations/parserEnum1.ts` | not supported | expected `,` or `}` after enum member, on ` IsStringIndexer = 1 << 1, ` |
| `parser/ecmascript5/EnumDeclarations/parserEnum2.ts` | not supported | expected `,` or `}` after enum member, on ` IsStringIndexer = 1 << 1, ` |
| `parser/ecmascript5/EnumDeclarations/parserEnum3.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/EnumDeclarations/parserEnum4.ts` | not supported | expected enum member name, on ` , ` |
| `parser/ecmascript5/EnumDeclarations/parserEnum5.ts` | not supported | expected `,` or `}` after enum member, on ` enum E3 { a: 1, } ` |
| `parser/ecmascript5/EnumDeclarations/parserEnum6.ts` | not supported | expected enum member name, on ` "A", "B", "C" ` |
| `parser/ecmascript5/EnumDeclarations/parserEnum7.ts` | not supported | expected enum member name, on ` 1, 2, 3 ` |
| `parser/ecmascript5/EnumDeclarations/parserEnumDeclaration1.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/EnumDeclarations/parserEnumDeclaration2.ts` | not supported | expected `;` after expression, on ` declare namespace M { ` |
| `parser/ecmascript5/EnumDeclarations/parserEnumDeclaration3.ts` | not supported | expected `;` after expression, on ` declare enum E { ` |
| `parser/ecmascript5/EnumDeclarations/parserEnumDeclaration4.ts` | porter failure | still pruning after 40 passes; our first unsupported error: `void` is a reserved keyword and can't be used as a name |
| `parser/ecmascript5/EnumDeclarations/parserEnumDeclaration5.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/EnumDeclarations/parserEnumDeclaration6.ts` | not supported | expected `,` or `}` after enum member, on ` C = 1 << 1, ` |
| `parser/ecmascript5/EnumDeclarations/parserInterfaceKeywordInEnum.ts` | not supported | `interface` is a reserved keyword and can't be used as a name, on ` interface, ` |
| `parser/ecmascript5/EnumDeclarations/parserInterfaceKeywordInEnum1.ts` | not supported | `interface` is a reserved keyword and can't be used as a name, on ` interface, ` |
| `parser/ecmascript5/ErrorRecovery/AccessibilityAfterStatic/parserAccessibilityAfterStatic1.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/ErrorRecovery/AccessibilityAfterStatic/parserAccessibilityAfterStatic10.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/ErrorRecovery/AccessibilityAfterStatic/parserAccessibilityAfterStatic11.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/ErrorRecovery/AccessibilityAfterStatic/parserAccessibilityAfterStatic14.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/ErrorRecovery/AccessibilityAfterStatic/parserAccessibilityAfterStatic2.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/ErrorRecovery/AccessibilityAfterStatic/parserAccessibilityAfterStatic3.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/ErrorRecovery/AccessibilityAfterStatic/parserAccessibilityAfterStatic4.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/ErrorRecovery/AccessibilityAfterStatic/parserAccessibilityAfterStatic5.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/ErrorRecovery/AccessibilityAfterStatic/parserAccessibilityAfterStatic6.ts` | the port changes what it checks | `tsc` then reports TS1068, TS7008 |
| `parser/ecmascript5/ErrorRecovery/AccessibilityAfterStatic/parserAccessibilityAfterStatic7.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/ErrorRecovery/ArgumentLists/parserErrorRecovery_ArgumentList1.ts` | not supported | `any` is not supported, on ` function foo(): any { ` |
| `parser/ecmascript5/ErrorRecovery/ArgumentLists/parserErrorRecovery_ArgumentList2.ts` | not supported | expected expression, on ` bar(; ` |
| `parser/ecmascript5/ErrorRecovery/ArgumentLists/parserErrorRecovery_ArgumentList3.ts` | not supported | expected expression, on ` return; ` |
| `parser/ecmascript5/ErrorRecovery/ArgumentLists/parserErrorRecovery_ArgumentList4.ts` | not supported | expected `,` or `)`, on ` return; ` |
| `parser/ecmascript5/ErrorRecovery/ArgumentLists/parserErrorRecovery_ArgumentList6.ts` | not supported | expected expression, on ` Foo(, ` |
| `parser/ecmascript5/ErrorRecovery/ArgumentLists/parserErrorRecovery_ArgumentList7.ts` | not supported | expected expression, on ` Foo(a,, ` |
| `parser/ecmascript5/ErrorRecovery/ArrayLiteralExpressions/parserErrorRecoveryArrayLiteralExpression1.ts` | not supported | expected `,` or `]`, on ` 4, 5, 6, 7]; ` |
| `parser/ecmascript5/ErrorRecovery/ArrayLiteralExpressions/parserErrorRecoveryArrayLiteralExpression2.ts` | not supported | expected field name after `.`, on ` .7042760848999023, 1.1955541372299194, 0.19600726664066315, -0.71200698614120... ` |
| `parser/ecmascript5/ErrorRecovery/ArrayLiteralExpressions/parserErrorRecoveryArrayLiteralExpression3.ts` | porter failure | nothing to prune at offsets 123, 123; our first unsupported error: expected `,` or `]` |
| `parser/ecmascript5/ErrorRecovery/ArrowFunctions/ArrowFunction1.ts` | not supported | expected type, on ` let v = (a: ) => { ` |
| `parser/ecmascript5/ErrorRecovery/ArrowFunctions/ArrowFunction3.ts` | porter failure | nothing to prune at offsets 32, 32; our first unsupported error: expected `;` after declaration |
| `parser/ecmascript5/ErrorRecovery/ArrowFunctions/ArrowFunction4.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/ErrorRecovery/ArrowFunctions/parserX_ArrowFunction1.ts` | not supported | expected type, on ` let v = (a: ) => { ` |
| `parser/ecmascript5/ErrorRecovery/ArrowFunctions/parserX_ArrowFunction3.ts` | porter failure | nothing to prune at offsets 32, 32; our first unsupported error: expected `;` after declaration |
| `parser/ecmascript5/ErrorRecovery/ArrowFunctions/parserX_ArrowFunction4.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/ErrorRecovery/Blocks/parserErrorRecovery_Block1.ts` | not supported | expected expression, on ` return; ` |
| `parser/ecmascript5/ErrorRecovery/Blocks/parserErrorRecovery_Block2.ts` | not supported | unexpected character `¬`, on ` ¬ ` |
| `parser/ecmascript5/ErrorRecovery/Blocks/parserErrorRecovery_Block3.ts` | not supported | expected `;` after expression, on ` private b(): boolean { ` |
| `parser/ecmascript5/ErrorRecovery/ClassElements/parserErrorRecovery_ClassElement1.ts` | porter failure | nothing to prune at offsets 165, 156; our first unsupported error: expected `:` and a type for the class field |
| `parser/ecmascript5/ErrorRecovery/ClassElements/parserErrorRecovery_ClassElement2.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `parser/ecmascript5/ErrorRecovery/ClassElements/parserErrorRecovery_ClassElement3.ts` | the port changes what it checks | `tsc` then reports TS1357 |
| `parser/ecmascript5/ErrorRecovery/Expressions/parserErrorRecovery_Expression1.ts` | not supported | expected expression, on ` let v = ()({}); ` |
| `parser/ecmascript5/ErrorRecovery/ExtendsOrImplementsClauses/parserErrorRecovery_ExtendsOrImplementsClause1.ts` | porter failure | nothing to prune at offsets 40; our first unsupported error: expected `{` after class header |
| `parser/ecmascript5/ErrorRecovery/ExtendsOrImplementsClauses/parserErrorRecovery_ExtendsOrImplementsClause2.ts` | not supported | expected `{` after class header, on ` class C extends A, { ` |
| `parser/ecmascript5/ErrorRecovery/ExtendsOrImplementsClauses/parserErrorRecovery_ExtendsOrImplementsClause3.ts` | not supported | expected type, on ` class C extends implements A { ` |
| `parser/ecmascript5/ErrorRecovery/ExtendsOrImplementsClauses/parserErrorRecovery_ExtendsOrImplementsClause4.ts` | porter failure | nothing to prune at offsets 53; our first unsupported error: expected `{` after class header |
| `parser/ecmascript5/ErrorRecovery/ExtendsOrImplementsClauses/parserErrorRecovery_ExtendsOrImplementsClause5.ts` | not supported | expected `{` after class header, on ` class C extends A, implements B, { ` |
| `parser/ecmascript5/ErrorRecovery/ExtendsOrImplementsClauses/parserErrorRecovery_ExtendsOrImplementsClause6.ts` | porter failure | nothing to prune at offsets 44; our first unsupported error: expected `{` after interface name |
| `parser/ecmascript5/ErrorRecovery/IfStatements/parserErrorRecoveryIfStatement1.ts` | not supported | expected expression, on ` } ` |
| `parser/ecmascript5/ErrorRecovery/IfStatements/parserErrorRecoveryIfStatement2.ts` | not supported | expected `)`, on ` } ` |
| `parser/ecmascript5/ErrorRecovery/IfStatements/parserErrorRecoveryIfStatement3.ts` | not supported | expected `)`, on ` } ` |
| `parser/ecmascript5/ErrorRecovery/IfStatements/parserErrorRecoveryIfStatement4.ts` | not supported | expected expression, on ` } ` |
| `parser/ecmascript5/ErrorRecovery/IfStatements/parserErrorRecoveryIfStatement5.ts` | the port changes what it checks | `tsc` then reports TS1068 |
| `parser/ecmascript5/ErrorRecovery/IfStatements/parserErrorRecoveryIfStatement6.ts` | not supported | expected `;` after expression, on ` public f2(): void { ` |
| `parser/ecmascript5/ErrorRecovery/IncompleteMemberVariables/parserErrorRecovery_IncompleteMemberVariable1.ts` | not supported | expected `;` after expression, on ` namespace Shapes { ` |
| `parser/ecmascript5/ErrorRecovery/IncompleteMemberVariables/parserErrorRecovery_IncompleteMemberVariable2.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/ErrorRecovery/LeftShifts/parserErrorRecovery_LeftShift1.ts` | porter failure | nothing to prune at offsets 47, 47; our first unsupported error: expected expression |
| `parser/ecmascript5/ErrorRecovery/ModuleElements/parserErrorRecovery_ModuleElement1.ts` | porter failure | nothing to prune at offsets 31, 45; our first unsupported error: expected expression |
| `parser/ecmascript5/ErrorRecovery/ModuleElements/parserErrorRecovery_ModuleElement2.ts` | porter failure | nothing to prune at offsets 71, 73; our first unsupported error: expected expression |
| `parser/ecmascript5/ErrorRecovery/ObjectLiterals/parserErrorRecovery_ObjectLiteral1.ts` | not supported | expected `,` or `}`, on ` let v = { a: 1 b: 2 } ` |
| `parser/ecmascript5/ErrorRecovery/ObjectLiterals/parserErrorRecovery_ObjectLiteral2.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `parser/ecmascript5/ErrorRecovery/ObjectLiterals/parserErrorRecovery_ObjectLiteral3.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `parser/ecmascript5/ErrorRecovery/ObjectLiterals/parserErrorRecovery_ObjectLiteral4.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `parser/ecmascript5/ErrorRecovery/ObjectLiterals/parserErrorRecovery_ObjectLiteral5.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `parser/ecmascript5/ErrorRecovery/ParameterLists/parserErrorRecovery_ParameterList1.ts` | the port changes what it checks | `tsc` then reports TS7010, TS7006, TS2389 |
| `parser/ecmascript5/ErrorRecovery/ParameterLists/parserErrorRecovery_ParameterList2.ts` | the port changes what it checks | `tsc` then reports TS7010, TS7006, TS2389 |
| `parser/ecmascript5/ErrorRecovery/ParameterLists/parserErrorRecovery_ParameterList4.ts` | not supported | unexpected character `¬`, on ` function f(a,¬): void { ` |
| `parser/ecmascript5/ErrorRecovery/ParameterLists/parserErrorRecovery_ParameterList5.ts` | not supported | expected `)`, on ` (a:number => { } ` |
| `parser/ecmascript5/ErrorRecovery/ParameterLists/parserErrorRecovery_ParameterList6.ts` | not supported | expected type, on ` public banana (x: break): void { } ` |
| `parser/ecmascript5/ErrorRecovery/parserCommaInTypeMemberList1.ts` | not supported | `any` is not supported, on ` let v: { workItem: any, width: string } = null as unknown as ({ workItem: any... ` |
| `parser/ecmascript5/ErrorRecovery/parserCommaInTypeMemberList2.ts` | the port changes what it checks | `tsc` then reports TS2581, TS7017 |
| `parser/ecmascript5/ErrorRecovery/parserEmptyParenthesizedExpression1.ts` | not supported | expected expression, on ` ().toString(); ` |
| `parser/ecmascript5/ErrorRecovery/parserEqualsGreaterThanAfterFunction1.ts` | the port changes what it checks | `tsc` then reports TS1144 |
| `parser/ecmascript5/ErrorRecovery/parserEqualsGreaterThanAfterFunction2.ts` | the port changes what it checks | `tsc` then reports TS1390 |
| `parser/ecmascript5/ErrorRecovery/parserErrantAccessibilityModifierInModule1.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `parser/ecmascript5/ErrorRecovery/parserErrantEqualsGreaterThanAfterFunction1.ts` | porter failure | nothing to prune at offsets 38, 38; our first unsupported error: `any` is not supported |
| `parser/ecmascript5/ErrorRecovery/parserErrantEqualsGreaterThanAfterFunction2.ts` | porter failure | nothing to prune at offsets 42, 42; our first unsupported error: `any` is not supported |
| `parser/ecmascript5/ErrorRecovery/parserErrantSemicolonInClass1.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/ErrorRecovery/parserFuzz1.ts` | porter failure | nothing to prune at offsets 27, 45; our first unsupported error: expected `;` after expression |
| `parser/ecmascript5/ErrorRecovery/parserMissingLambdaOpenBrace1.ts` | the port changes what it checks | `tsc` then reports TS1068, TS1128, TS7006, TS2582 |
| `parser/ecmascript5/ErrorRecovery/parserModifierOnPropertySignature1.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/ErrorRecovery/parserModifierOnPropertySignature2.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/ErrorRecovery/parserModifierOnStatementInBlock1.ts` | the port changes what it checks | `tsc` then reports TS2683 |
| `parser/ecmascript5/ErrorRecovery/parserModifierOnStatementInBlock2.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/ErrorRecovery/parserModifierOnStatementInBlock3.ts` | not supported | `export` statements must appear at the top of the file, on ` export function bar(): void { ` |
| `parser/ecmascript5/ErrorRecovery/parserModifierOnStatementInBlock4.ts` | not supported | `export` statements must appear at the top of the file, on ` export function bar(): void { ` |
| `parser/ecmascript5/ErrorRecovery/parserPublicBreak1.ts` | not supported | expected `;` after expression, on ` public break; ` |
| `parser/ecmascript5/ErrorRecovery/parserStatementIsNotAMemberVariableDeclaration1.ts` | not supported | parameter `key` requires a type annotation, on ` "set": function (key, value) { ` |
| `parser/ecmascript5/ErrorRecovery/parserUnfinishedTypeNameBeforeKeyword1.ts` | not supported | expected identifier after `.` in type name, on ` let x: TypeModule1. = null as unknown as (TypeModule1.); ` |
| `parser/ecmascript5/ErrorRecovery/parserUnterminatedGeneric1.ts` | not supported | `any` is not supported, on ` all(promises: IPromise < any > []): IPromise< ` |
| `parser/ecmascript5/ErrorRecovery/parserUnterminatedGeneric2.ts` | not supported | expected `;` after expression, on ` declare namespace ng { ` |
| `parser/ecmascript5/ErrorRecovery/SourceUnits/parserErrorRecovery_SourceUnit1.ts` | porter failure | nothing to prune at offsets 31; our first unsupported error: expected expression |
| `parser/ecmascript5/ErrorRecovery/SwitchStatements/parserErrorRecovery_SwitchStatement1.ts` | not supported | expected expression, on ` case 2: ` |
| `parser/ecmascript5/ErrorRecovery/SwitchStatements/parserErrorRecovery_SwitchStatement2.ts` | not supported | expected `case` or `default` at the start of a `switch` body, on ` class D { ` |
| `parser/ecmascript5/ErrorRecovery/TypeArgumentLists/parserX_TypeArgumentList1.ts` | porter failure | nothing to prune at offsets 27; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/ErrorRecovery/TypeArgumentLists/TypeArgumentList1.ts` | porter failure | nothing to prune at offsets 27; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/ErrorRecovery/VariableLists/parserErrorRecovery_VariableList1.ts` | not supported | `let` declaration requires an initializer, on ` let a, ` |
| `parser/ecmascript5/ErrorRecovery/VariableLists/parserInvalidIdentifiersInVariableStatements1.ts` | the port changes what it checks | `tsc` then reports TS1440, TS1128, TS2304 |
| `parser/ecmascript5/ErrorRecovery/VariableLists/parserVariableStatement1.ts` | not supported | `let` declaration requires an initializer, on ` let a, ` |
| `parser/ecmascript5/ErrorRecovery/VariableLists/parserVariableStatement2.ts` | not supported | `let` declaration requires an initializer, on ` , b ` |
| `parser/ecmascript5/ErrorRecovery/VariableLists/parserVariableStatement3.ts` | not supported | expected identifier after `let`/`const`, on ` a, ` |
| `parser/ecmascript5/ErrorRecovery/VariableLists/parserVariableStatement4.ts` | not supported | expected identifier after `let`/`const`, on ` a ` |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment1.ts` | the port changes what it checks | `tsc` then reports TS1203 |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment2.ts` | the port changes what it checks | `tsc` then reports TS1203 |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment3.ts` | the port changes what it checks | `tsc` then reports TS1203 |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment4.ts` | not supported | expected a declaration after `export`, on ` export = ; ` |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment5.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment6.ts` | not supported | expected `;` after expression, on ` declare module "M" { ` |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment7.ts` | the port changes what it checks | `tsc` then reports TS1203 |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment8.ts` | the port changes what it checks | `tsc` then reports TS1203 |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment9.ts` | not supported | expected `;` after expression, on ` namespace Foo { ` |
| `parser/ecmascript5/Expressions/parseIncompleteBinaryExpression1.ts` | not supported | expected expression, on ` let v = \|\| b; ` |
| `parser/ecmascript5/Expressions/parserAssignmentExpression1.ts` | not supported | invalid assignment target, on ` (foo()) = bar; ` |
| `parser/ecmascript5/Expressions/parserConditionalExpression1.ts` | not supported | expected `)`, on ` (a=this.R[c])?a.JW\|\|(a.e5(this,c),a.JW=_.l):this.A ` |
| `parser/ecmascript5/Expressions/parserInvocationOfMemberAccessOffOfObjectCreationExpression1.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/Expressions/parserMemberAccessAfterPostfixExpression1.ts` | porter failure | nothing to prune at offsets 35; our first unsupported error: expected `;` after expression |
| `parser/ecmascript5/Expressions/parserObjectCreation2.ts` | not supported | expected expression, on ` new new Foo()() ` |
| `parser/ecmascript5/Expressions/parserPostfixPostfixExpression1.ts` | not supported | expected `;` after expression, on ` a++ ++; ` |
| `parser/ecmascript5/Expressions/parserPostfixUnaryExpression1.ts` | not supported | expected `;` after expression, on ` foo ++ ++; ` |
| `parser/ecmascript5/Expressions/parserTypeAssertionInObjectCreationExpression1.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `parser/ecmascript5/Expressions/parserUnaryExpression1.ts` | not supported | expected expression, on ` ++this; ` |
| `parser/ecmascript5/Expressions/parserUnaryExpression2.ts` | not supported | expected expression, on ` ++function(e) { } ` |
| `parser/ecmascript5/Expressions/parserUnaryExpression3.ts` | not supported | expected expression, on ` ++[0]; ` |
| `parser/ecmascript5/Expressions/parserUnaryExpression4.ts` | not supported | expected expression, on ` ++{}; ` |
| `parser/ecmascript5/Expressions/parserUnaryExpression5.ts` | not supported | expected expression, on ` ++ delete foo.bar ` |
| `parser/ecmascript5/Expressions/parserUnaryExpression6.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/Expressions/parserUnaryExpression7.ts` | not supported | expected expression, on ` ++ new Foo(); ` |
| `parser/ecmascript5/FunctionDeclarations/parserFunctionDeclaration1.ts` | the port changes what it checks | `tsc` then reports TS1183 |
| `parser/ecmascript5/FunctionDeclarations/parserFunctionDeclaration2.ts` | the port changes what it checks | `tsc` then reports TS1128 |
| `parser/ecmascript5/FunctionDeclarations/parserFunctionDeclaration3.ts` | the port changes what it checks | `tsc` then reports TS2389 |
| `parser/ecmascript5/FunctionDeclarations/parserFunctionDeclaration4.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/FunctionDeclarations/parserFunctionDeclaration5.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/FunctionDeclarations/parserFunctionDeclaration6.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/FunctionDeclarations/parserFunctionDeclaration7.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/FunctionDeclarations/parserFunctionDeclaration8.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/Fuzz/parser0_004152.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/Fuzz/parser768531.ts` | not supported | expected `;` after expression, on ` {a: 3} ` |
| `parser/ecmascript5/Generics/parserAmbiguity1.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/Generics/parserAmbiguity2.ts` | checks too little | 3 after the port |
| `parser/ecmascript5/Generics/parserAmbiguity3.ts` | checks too little | 4 after the port |
| `parser/ecmascript5/Generics/parserAmbiguityWithBinaryOperator1.ts` | the port changes what it checks | `tsc` then reports TS18048 |
| `parser/ecmascript5/Generics/parserAmbiguityWithBinaryOperator2.ts` | the port changes what it checks | `tsc` then reports TS18048 |
| `parser/ecmascript5/Generics/parserAmbiguityWithBinaryOperator3.ts` | the port changes what it checks | `tsc` then reports TS18048 |
| `parser/ecmascript5/Generics/parserAmbiguityWithBinaryOperator4.ts` | not supported | `let` declaration requires an initializer, on ` let a, b, c; ` |
| `parser/ecmascript5/Generics/parserCastVersusArrowFunction1.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `parser/ecmascript5/Generics/parserConstructorAmbiguity1.ts` | not supported | expected `(` after constructor name in `new` expression, on ` new Date<A; ` |
| `parser/ecmascript5/Generics/parserConstructorAmbiguity2.ts` | not supported | expected `(` after constructor name in `new` expression, on ` new Date<A ` |
| `parser/ecmascript5/Generics/parserConstructorAmbiguity3.ts` | porter failure | nothing to prune at offsets 32; our first unsupported error: expected `(` after constructor name in `new` expression |
| `parser/ecmascript5/Generics/parserConstructorAmbiguity4.ts` | not supported | expected `(` after constructor name in `new` expression, on ` new Date<A ? B : C ` |
| `parser/ecmascript5/Generics/parserGenericClass1.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/Generics/parserGenericClass2.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/Generics/parserGenericConstraint1.ts` | not supported | expected `,` or `>`, on ` class C<T extends number> { ` |
| `parser/ecmascript5/Generics/parserGenericConstraint2.ts` | not supported | expected `,` or `>`, on ` class C<T extends List<T> > { ` |
| `parser/ecmascript5/Generics/parserGenericConstraint3.ts` | not supported | expected `,` or `>`, on ` class C<T extends List<T>> { ` |
| `parser/ecmascript5/Generics/parserGenericConstraint4.ts` | not supported | expected `,` or `>`, on ` class C<T extends List<List<T> > > { ` |
| `parser/ecmascript5/Generics/parserGenericConstraint5.ts` | not supported | expected `,` or `>`, on ` class C<T extends List<List<T>> > { ` |
| `parser/ecmascript5/Generics/parserGenericConstraint6.ts` | not supported | expected `,` or `>`, on ` class C<T extends List<List<T> >> { ` |
| `parser/ecmascript5/Generics/parserGenericConstraint7.ts` | not supported | expected `,` or `>`, on ` class C<T extends List<List<T>>> { ` |
| `parser/ecmascript5/Generics/parserGenericsInInterfaceDeclaration1.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `parser/ecmascript5/Generics/parserGenericsInVariableDeclaration1.ts` | not supported | expected `,` or `>` to close generic argument list, on ` let v_2 : Foo<T>= 1; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity1.ts` | not supported | expected expression, on ` 1 >> 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity10.ts` | not supported | expected expression, on ` >>> // after ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity11.ts` | not supported | expected expression, on ` 1 >>= 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity12.ts` | porter failure | nothing to prune at offsets 24, 24; our first unsupported error: expected expression |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity13.ts` | porter failure | nothing to prune at offsets 27, 27; our first unsupported error: expected expression |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity14.ts` | porter failure | nothing to prune at offsets 24, 24; our first unsupported error: expected expression |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity15.ts` | not supported | expected expression, on ` >>= // after ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity16.ts` | not supported | expected expression, on ` 1 >>>= 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity17.ts` | porter failure | nothing to prune at offsets 25, 25; our first unsupported error: expected expression |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity18.ts` | porter failure | nothing to prune at offsets 28, 28; our first unsupported error: expected expression |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity19.ts` | porter failure | nothing to prune at offsets 25, 25; our first unsupported error: expected expression |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity2.ts` | not supported | expected expression, on ` 1 > > 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity20.ts` | not supported | expected expression, on ` >>>= // after ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity3.ts` | not supported | expected expression, on ` 1 >/**/> 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity4.ts` | not supported | expected expression, on ` > 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity5.ts` | not supported | expected expression, on ` >> // after ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity6.ts` | not supported | expected expression, on ` 1 >>> 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity7.ts` | not supported | expected expression, on ` 1 >> > 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity8.ts` | not supported | expected expression, on ` 1 >>/**/> 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity9.ts` | not supported | expected expression, on ` 1 >> ` |
| `parser/ecmascript5/Generics/parserMemberAccessExpression1.ts` | not supported | expected expression, on ` Foo<T>.Bar(); ` |
| `parser/ecmascript5/Generics/parserMemberAccessOffOfGenericType1.ts` | not supported | expected expression, on ` let v = List<number>.makeChild(); ` |
| `parser/ecmascript5/Generics/parserObjectCreation1.ts` | checks too little | 3 after the port |
| `parser/ecmascript5/IndexMemberDeclarations/` | not supported | index signatures |
| `parser/ecmascript5/IndexSignatures/` | not supported | index signatures |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration1.ts` | not supported | expected `{` after interface name, on ` interface I extends A extends B { ` |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration2.ts` | not supported | expected `{` after interface name, on ` interface I implements A { ` |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration3.ts` | not supported | expected `;` after expression, on ` public interface I { ` |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration4.ts` | not supported | expected `;` after expression, on ` static interface I { ` |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration5.ts` | not supported | expected `;` after expression, on ` declare interface I { ` |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration6.ts` | not supported | expected a declaration after `export`, on ` export export interface I { ` |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration7.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration8.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration9.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` get foo(): number, ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessor1.ts` | not supported | parameter requires a type annotation, on ` set foo(a) { } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration1.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration10.ts` | not supported | expected `:` and a type for the class field, on ` export get Foo() { } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration11.ts` | not supported | expected `:` and a type for the class field, on ` declare get Foo() { } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration12.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration13.ts` | the port changes what it checks | `tsc` then reports TS7032 |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration14.ts` | not supported | expected expression, on ` } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration15.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration16.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration17.ts` | not supported | optional function parameters are not yet supported, on ` set Foo(a?: number) { } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration18.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7019 |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration2.ts` | not supported | expected `:` and a type for the class field, on ` get "b"() { } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration3.ts` | not supported | expected `:` and a type for the class field, on ` get 0() { } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration4.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration5.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration6.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration7.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration8.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration9.ts` | not supported | static accessors are not supported, on ` static public get Foo() { } ` |
| `parser/ecmascript5/MemberFunctionDeclarations/parserMemberFunctionDeclaration1.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/MemberFunctionDeclarations/parserMemberFunctionDeclaration2.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/MemberFunctionDeclarations/parserMemberFunctionDeclaration3.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/MemberFunctionDeclarations/parserMemberFunctionDeclaration4.ts` | not supported | expected `:` and a type for the class field, on ` export Foo(): void { } ` |
| `parser/ecmascript5/MemberFunctionDeclarations/parserMemberFunctionDeclaration5.ts` | not supported | expected `:` and a type for the class field, on ` declare Foo(): void { } ` |
| `parser/ecmascript5/MemberVariableDeclarations/parserMemberVariableDeclaration1.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/MemberVariableDeclarations/parserMemberVariableDeclaration2.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/MemberVariableDeclarations/parserMemberVariableDeclaration3.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/MemberVariableDeclarations/parserMemberVariableDeclaration4.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/MemberVariableDeclarations/parserMemberVariableDeclaration5.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature1.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature10.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature11.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature12.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature2.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature3.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature4.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature5.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature6.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature7.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature8.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature9.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/MissingTokens/parserMissingToken1.ts` | not supported | expected expression, on ` a / finally ` |
| `parser/ecmascript5/MissingTokens/parserMissingToken2.ts` | not supported | unterminated regex literal, on ` / b; ` |
| `parser/ecmascript5/ModuleDeclarations/parserModule1.ts` | not supported | expected a declaration after `export`, on ` export namespace CompilerDiagnostics { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration1.ts` | not supported | expected `;` after expression, on ` module "Foo" { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration10.ts` | the port changes what it checks | `tsc` then reports TS2389 |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration11.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration12.ts` | not supported | expected `;` after expression, on ` namespace A.string { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration2.ts` | not supported | expected `;` after expression, on ` declare module "Foo" { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration3.ts` | not supported | expected `;` after expression, on ` declare namespace M { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration4.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration5.ts` | not supported | expected `;` after expression, on ` namespace M1 { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration6.ts` | not supported | expected `;` after expression, on ` namespace number { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration7.ts` | not supported | expected `;` after expression, on ` namespace number.a { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration8.ts` | not supported | expected `;` after expression, on ` namespace a.number { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration9.ts` | not supported | expected `;` after expression, on ` namespace a.number.b { ` |
| `parser/ecmascript5/ObjectLiterals/parserObjectLiterals1.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/ObjectTypes/parserObjectType1.ts` | checks too little | 3 after the port |
| `parser/ecmascript5/ObjectTypes/parserObjectType2.ts` | checks too little | 3 after the port |
| `parser/ecmascript5/ObjectTypes/parserObjectType3.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/ObjectTypes/parserObjectType4.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/ObjectTypes/parserObjectType5.ts` | the port changes what it checks | `tsc` then reports TS7020 |
| `parser/ecmascript5/ParameterLists/parserParameterList1.ts` | the port changes what it checks | `tsc` then reports TS7019, TS7006 |
| `parser/ecmascript5/ParameterLists/parserParameterList10.ts` | not supported | expected expression, on ` } ` |
| `parser/ecmascript5/ParameterLists/parserParameterList11.ts` | not supported | expected `,` or `)`, on ` (...arg?) => 102; ` |
| `parser/ecmascript5/ParameterLists/parserParameterList12.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/ParameterLists/parserParameterList13.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `parser/ecmascript5/ParameterLists/parserParameterList14.ts` | not supported | expected `;` after expression, on ` declare class C { ` |
| `parser/ecmascript5/ParameterLists/parserParameterList15.ts` | the port changes what it checks | `tsc` then reports TS7010, TS7006 |
| `parser/ecmascript5/ParameterLists/parserParameterList16.ts` | the port changes what it checks | `tsc` then reports TS7010, TS7006 |
| `parser/ecmascript5/ParameterLists/parserParameterList17.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/ParameterLists/parserParameterList2.ts` | not supported | optional function parameters are not yet supported, on ` F(A?: number= 0): void { } ` |
| `parser/ecmascript5/ParameterLists/parserParameterList3.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/ParameterLists/parserParameterList4.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/ParameterLists/parserParameterList5.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/ParameterLists/parserParameterList6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/ParameterLists/parserParameterList7.ts` | not supported | expected `{`, on ` constructor(public p1:string); // ERROR ` |
| `parser/ecmascript5/ParameterLists/parserParameterList8.ts` | not supported | expected `;` after expression, on ` declare class C2 { ` |
| `parser/ecmascript5/ParameterLists/parserParameterList9.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `parser/ecmascript5/parser10.1.1-8gs.ts` | checks too little | 3 after the port |
| `parser/ecmascript5/parser15.4.4.14-9-2.ts` | the port changes what it checks | `tsc` then reports TS2366 |
| `parser/ecmascript5/parserAdditiveExpression1.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/parserArgumentList1.ts` | not supported | unknown type `HTMLElement`, on ` export function removeClass (node:HTMLElement, className:string): void { ` |
| `parser/ecmascript5/parserAstSpans1.ts` | not supported | expected `:` and a type for the class field, on ` public i1_p1!: number; ` |
| `parser/ecmascript5/parserDebuggerStatement1.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/parserDebuggerStatement2.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/parserEmptyFile1.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/parserEmptyStatement1.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/parserExportAsFunctionIdentifier.ts` | not supported | `as` to `Foo` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let f: Foo = null as unknown as (Foo); ` |
| `parser/ecmascript5/parserImportDeclaration1.ts` | not supported | expected `from` after import specifier list, on ` import TypeScript = TypeScriptServices.TypeScript; ` |
| `parser/ecmascript5/parserInExpression1.ts` | not supported | expected `,` or `}`, on ` console.log("a" in { "a": true }); ` |
| `parser/ecmascript5/parserKeywordsAsIdentifierName1.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/parserKeywordsAsIdentifierName2.ts` | porter failure | nothing to prune at offsets 98; our first unsupported error: unterminated block comment |
| `parser/ecmascript5/parserNoASIOnCallAfterFunctionExpression1.ts` | not supported | `any` is not supported, on ` (<any>window).foo; ` |
| `parser/ecmascript5/parserNotRegex1.ts` | checks too little | 4 after the port |
| `parser/ecmascript5/parserNotRegex2.ts` | not supported | `any` is not supported, on ` const A: any = null as unknown as (any); ` |
| `parser/ecmascript5/parserObjectCreationArrayLiteral1.ts` | not supported | expected `(` after constructor name in `new` expression, on ` new Foo[]; ` |
| `parser/ecmascript5/parserObjectCreationArrayLiteral2.ts` | not supported | expected `(` after constructor name in `new` expression, on ` new Foo[1]; ` |
| `parser/ecmascript5/parserObjectCreationArrayLiteral3.ts` | not supported | expected `(` after constructor name in `new` expression, on ` new Foo[](); ` |
| `parser/ecmascript5/parserObjectCreationArrayLiteral4.ts` | not supported | expected `(` after constructor name in `new` expression, on ` new Foo[1](); ` |
| `parser/ecmascript5/parserOptionalTypeMembers1.ts` | not supported | `any` is not supported, on ` value?: any; ` |
| `parser/ecmascript5/parserOverloadOnConstants1.ts` | not supported | unknown type `HTMLElement`, on ` createElement(tagName: string): HTMLElement; ` |
| `parser/ecmascript5/parserParenthesizedVariableAndFunctionInTernary.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `parser/ecmascript5/parserParenthesizedVariableAndParenthesizedFunctionInTernary.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `parser/ecmascript5/parserRealSource1.ts` | the port changes what it checks | `tsc` then reports TS2322, TS2345 |
| `parser/ecmascript5/parserRealSource10.ts` | the port changes what it checks | `tsc` then reports TS7053, TS7006 |
| `parser/ecmascript5/parserRealSource11.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7053, TS7006, TS2345 |
| `parser/ecmascript5/parserRealSource12.ts` | the port changes what it checks | `tsc` then reports TS2345 |
| `parser/ecmascript5/parserRealSource13.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7053 |
| `parser/ecmascript5/parserRealSource14.ts` | not supported | expected `;` after expression, on ` namespace TypeScript { ` |
| `parser/ecmascript5/parserRealSource2.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/parserRealSource3.ts` | not supported | expected `;` after expression, on ` namespace TypeScript { ` |
| `parser/ecmascript5/parserRealSource4.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7006, TS7053 |
| `parser/ecmascript5/parserRealSource5.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/parserRealSource6.ts` | not supported | expected `;` after expression, on ` namespace TypeScript { ` |
| `parser/ecmascript5/parserRealSource7.ts` | not supported | unexpected character `&`, on ` (varDecl.varFlags & VarFlags.Readonly) == VarFlags.None, ` |
| `parser/ecmascript5/parserRealSource8.ts` | not supported | unexpected character `&`, on ` if (!(instType.typeFlags & TypeFlags.IsClass) && !hasFlag(funcDecl.fncFlags, ... ` |
| `parser/ecmascript5/parserRealSource9.ts` | the port changes what it checks | `tsc` then reports TS2322, TS7006 |
| `parser/ecmascript5/parserS12.11_A3_T4.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/parserS7.2_A1.5_T2.ts` | the port changes what it checks | `tsc` then reports TS2448 |
| `parser/ecmascript5/parserS7.3_A1.1_T2.ts` | not supported | expected identifier after `let`/`const`, on ` x ` |
| `parser/ecmascript5/parserS7.6_A4.2_T1.ts` | not supported | unexpected character `\`, on ` let \u0410 = 1; ` |
| `parser/ecmascript5/parserS7.6.1.1_A1.10.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/parserS7.9_A5.7_T1.ts` | not supported | expected `;` after declaration, on ` let x=0, y=0; ` |
| `parser/ecmascript5/parserSbp_7.9_A9_T3.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/parserSyntaxWalker.generated.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/parserUnicode1.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `parser/ecmascript5/parserUnicode2.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/parserUnicode3.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/parserUnicodeWhitespaceCharacter1.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/parserUsingConstructorAsIdentifier.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/parserVoidExpression1.ts` | not supported | expected expression, on ` void 0; ` |
| `parser/ecmascript5/parservoidInQualifiedName0.ts` | not supported | cannot cast `unknown` to `void`: no assignable direction between these types, on ` let v : void = null as unknown as (void); ` |
| `parser/ecmascript5/parservoidInQualifiedName1.ts` | not supported | expected `;` after declaration, on ` let v : void = null as unknown as (void).x; ` |
| `parser/ecmascript5/parservoidInQualifiedName2.ts` | not supported | expected identifier after `.` in type name, on ` let v : x.void = null as unknown as (x.void); ` |
| `parser/ecmascript5/PropertyAssignments/parserFunctionPropertyAssignment1.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/PropertyAssignments/parserFunctionPropertyAssignment2.ts` | not supported | expected field name, on ` let v = { 0() { } }; ` |
| `parser/ecmascript5/PropertyAssignments/parserFunctionPropertyAssignment3.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/PropertyAssignments/parserFunctionPropertyAssignment4.ts` | not supported | expected field name, on ` let v = { 0<T>() { } }; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature1.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature10.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature11.ts` | not supported | expected interface member name, on ` 2:any; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature12.ts` | not supported | expected interface member name, on ` 3?:any; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature2.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature3.ts` | not supported | `any` is not supported, on ` C:any; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature4.ts` | not supported | `any` is not supported, on ` D?:any; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature5.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature6.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature7.ts` | not supported | `any` is not supported, on ` "G":any; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature8.ts` | not supported | `any` is not supported, on ` "H"?:any; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature9.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/Protected/Protected1.ts` | not supported | expected `;` after expression, on ` protected class C { ` |
| `parser/ecmascript5/Protected/Protected2.ts` | not supported | expected `;` after expression, on ` protected namespace M { ` |
| `parser/ecmascript5/Protected/Protected3.ts` | not supported | `protected` is not supported, on ` protected constructor() { } ` |
| `parser/ecmascript5/Protected/Protected4.ts` | not supported | `protected` is not supported, on ` protected public m(): void { } ` |
| `parser/ecmascript5/Protected/Protected5.ts` | not supported | `protected` is not supported, on ` protected static m(): void { } ` |
| `parser/ecmascript5/Protected/Protected6.ts` | not supported | expected `:` and a type for the class field, on ` static protected m(): void { } ` |
| `parser/ecmascript5/Protected/Protected7.ts` | not supported | `protected` is not supported, on ` protected private m(): void { } ` |
| `parser/ecmascript5/Protected/Protected8.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/Protected/Protected9.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/RealWorld/parserharness.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `parser/ecmascript5/RealWorld/parserindenter.ts` | not supported | expected `;` after expression, on ` namespace Formatting { ` |
| `parser/ecmascript5/RegressionTests/parser509534.ts` | the port changes what it checks | `tsc` then reports TS2580, TS7006 |
| `parser/ecmascript5/RegressionTests/parser509546_1.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/RegressionTests/parser509546_2.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/RegressionTests/parser509546.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/RegressionTests/parser509618.ts` | not supported | expected `;` after expression, on ` declare namespace ambiModule { ` |
| `parser/ecmascript5/RegressionTests/parser509630.ts` | not supported | unknown type `Type`, on ` class Any extends Type { ` |
| `parser/ecmascript5/RegressionTests/parser509667.ts` | not supported | expected field name after `.`, on ` } ` |
| `parser/ecmascript5/RegressionTests/parser509668.ts` | not supported | parameter requires a type annotation, on ` constructor (public ...args: string[]) { } ` |
| `parser/ecmascript5/RegressionTests/parser509669.ts` | not supported | `any` is not supported, on ` function foo():any { ` |
| `parser/ecmascript5/RegressionTests/parser509677.ts` | checks too little | 3 after the port |
| `parser/ecmascript5/RegressionTests/parser509693.ts` | the port changes what it checks | `tsc` then reports TS2580 |
| `parser/ecmascript5/RegressionTests/parser509698.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/RegressionTests/parser512084.ts` | the port changes what it checks | `tsc` then reports TS1068 |
| `parser/ecmascript5/RegressionTests/parser512097.ts` | not supported | expected `,` or `}`, on ` let tt = { aa; } ` |
| `parser/ecmascript5/RegressionTests/parser512325.ts` | porter failure | nothing to prune at offsets 40, 40; our first unsupported error: expected parameter name |
| `parser/ecmascript5/RegressionTests/parser519458.ts` | the port changes what it checks | `tsc` then reports TS2580 |
| `parser/ecmascript5/RegressionTests/parser521128.ts` | the port changes what it checks | `tsc` then reports TS2580 |
| `parser/ecmascript5/RegressionTests/parser553699.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/RegressionTests/parser566700.ts` | not supported | expected expression, on ` let v = ()({}); ` |
| `parser/ecmascript5/RegressionTests/parser579071.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/RegressionTests/parser585151.ts` | not supported | expected `:` and a type for the class field, on ` let icecream = "chocolate"; ` |
| `parser/ecmascript5/RegressionTests/parser596700.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/RegressionTests/parser618973.ts` | not supported | expected a declaration after `export`, on ` export export class Foo { ` |
| `parser/ecmascript5/RegressionTests/parser642331_1.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/RegressionTests/parser642331.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/RegressionTests/parser643728.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/RegressionTests/parser645086_1.ts` | not supported | expected `;` after declaration, on ` let v = /[]/]/ ` |
| `parser/ecmascript5/RegressionTests/parser645086_2.ts` | not supported | expected `;` after declaration, on ` let v = /[^]/]/ ` |
| `parser/ecmascript5/RegressionTests/parser645086_3.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/RegressionTests/parser645086_4.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/RegressionTests/parser645484.ts` | not supported | `any` is not supported, on ` new?(): any; ` |
| `parser/ecmascript5/RegressionTests/parserNotHexLiteral1.ts` | checks too little | 4 after the port |
| `parser/ecmascript5/RegressionTests/parserTernaryAndCommaOperators1.ts` | not supported | expected `;` after expression, on ` b.src ? 1 : 2, c && d; ` |
| `parser/ecmascript5/RegularExpressions/parserRegularExpression1.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/RegularExpressions/parserRegularExpression2.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/RegularExpressions/parserRegularExpression3.ts` | checks too little | 3 after the port |
| `parser/ecmascript5/RegularExpressions/parserRegularExpression4.ts` | not supported | unexpected character `\`, on ` if (Ca.test(c.href) \|\| Ba.test(c.href) && /(\\?\|&)adurl=/.test(c.href) && !/(... ` |
| `parser/ecmascript5/RegularExpressions/parserRegularExpression5.ts` | not supported | unexpected character `\`, on ` if (a) / (\\ ? \| & ) rct = j / .test(c.href); ` |
| `parser/ecmascript5/RegularExpressions/parserRegularExpression6.ts` | the port changes what it checks | `tsc` then reports TS18048 |
| `parser/ecmascript5/RegularExpressions/parserRegularExpressionDivideAmbiguity1.ts` | checks too little | 3 after the port |
| `parser/ecmascript5/RegularExpressions/parserRegularExpressionDivideAmbiguity2.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/RegularExpressions/parserRegularExpressionDivideAmbiguity3.ts` | not supported | expected expression, on ` if (1) /regexp/a.foo(); ` |
| `parser/ecmascript5/RegularExpressions/parserRegularExpressionDivideAmbiguity4.ts` | not supported | unterminated regex literal, on ` foo(/notregexp); ` |
| `parser/ecmascript5/RegularExpressions/parserRegularExpressionDivideAmbiguity5.ts` | checks too little | 3 after the port |
| `parser/ecmascript5/RegularExpressions/parserRegularExpressionDivideAmbiguity6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/RegularExpressions/parserRegularExpressionDivideAmbiguity7.ts` | porter failure | nothing to prune at offsets 26; our first unsupported error: expected `;` after expression |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens1.ts` | porter failure | nothing to prune at offsets 19; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens10.ts` | porter failure | nothing to prune at offsets 19, 21; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens11.ts` | porter failure | nothing to prune at offsets 21, 23, 25; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens12.ts` | porter failure | nothing to prune at offsets 19, 21, 23; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens13.ts` | porter failure | nothing to prune at offsets 28; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens14.ts` | porter failure | nothing to prune at offsets 19, 42; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens15.ts` | porter failure | nothing to prune at offsets 39, 41; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens16.ts` | porter failure | nothing to prune at offsets 61, 24, 71, 24; our first unsupported error: unexpected character `¬` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens17.ts` | not supported | unexpected character `\`, on ` foo(a, \ ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens18.ts` | not supported | unexpected character `\`, on ` foo(a \ ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens19.ts` | porter failure | nothing to prune at offsets 19; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens2.ts` | porter failure | nothing to prune at offsets 19, 20; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens20.ts` | the port changes what it checks | `tsc` then reports TS1005 |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens3.ts` | porter failure | nothing to prune at offsets 19, 23; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens4.ts` | porter failure | nothing to prune at offsets 19; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens5.ts` | porter failure | nothing to prune at offsets 19; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens6.ts` | porter failure | nothing to prune at offsets 27; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens7.ts` | porter failure | nothing to prune at offsets 27; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens8.ts` | porter failure | nothing to prune at offsets 29; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens9.ts` | porter failure | nothing to prune at offsets 48; our first unsupported error: unexpected character `\` |
| `parser/ecmascript5/Statements/BreakStatements/parser_breakInIterationOrSwitchStatement1.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/Statements/BreakStatements/parser_breakInIterationOrSwitchStatement2.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/Statements/BreakStatements/parser_breakInIterationOrSwitchStatement3.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/Statements/BreakStatements/parser_breakInIterationOrSwitchStatement4.ts` | not supported | `let` declaration requires an initializer, on ` for (let i in something) { ` |
| `parser/ecmascript5/Statements/BreakStatements/parser_breakNotInIterationOrSwitchStatement1.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/Statements/BreakStatements/parser_breakNotInIterationOrSwitchStatement2.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/Statements/BreakStatements/parser_breakTarget1.ts` | not supported | expected `;` after expression, on ` target: ` |
| `parser/ecmascript5/Statements/BreakStatements/parser_breakTarget2.ts` | not supported | expected `;` after expression, on ` target: ` |
| `parser/ecmascript5/Statements/BreakStatements/parser_breakTarget3.ts` | not supported | expected `;` after expression, on ` target1: ` |
| `parser/ecmascript5/Statements/BreakStatements/parser_breakTarget4.ts` | not supported | expected `;` after expression, on ` target1: ` |
| `parser/ecmascript5/Statements/BreakStatements/parser_breakTarget5.ts` | not supported | expected `;` after expression, on ` target: ` |
| `parser/ecmascript5/Statements/BreakStatements/parser_breakTarget6.ts` | not supported | expected `;` after `break`, on ` break target; ` |
| `parser/ecmascript5/Statements/ContinueStatements/parser_continueInIterationStatement1.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/Statements/ContinueStatements/parser_continueInIterationStatement2.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/Statements/ContinueStatements/parser_continueInIterationStatement3.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/Statements/ContinueStatements/parser_continueInIterationStatement4.ts` | not supported | `let` declaration requires an initializer, on ` for (let i in something) { ` |
| `parser/ecmascript5/Statements/ContinueStatements/parser_continueLabel.ts` | not supported | expected `;` after expression, on ` label1: for(let i = 0; i < 1; i++) { ` |
| `parser/ecmascript5/Statements/ContinueStatements/parser_continueNotInIterationStatement1.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/Statements/ContinueStatements/parser_continueNotInIterationStatement2.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/Statements/ContinueStatements/parser_continueNotInIterationStatement3.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/Statements/ContinueStatements/parser_continueNotInIterationStatement4.ts` | not supported | expected `;` after expression, on ` TWO: ` |
| `parser/ecmascript5/Statements/ContinueStatements/parser_continueTarget1.ts` | not supported | expected `;` after expression, on ` target: ` |
| `parser/ecmascript5/Statements/ContinueStatements/parser_continueTarget2.ts` | not supported | expected `;` after expression, on ` target: ` |
| `parser/ecmascript5/Statements/ContinueStatements/parser_continueTarget3.ts` | not supported | expected `;` after expression, on ` target1: ` |
| `parser/ecmascript5/Statements/ContinueStatements/parser_continueTarget4.ts` | not supported | expected `;` after expression, on ` target1: ` |
| `parser/ecmascript5/Statements/ContinueStatements/parser_continueTarget5.ts` | not supported | expected `;` after expression, on ` target: ` |
| `parser/ecmascript5/Statements/ContinueStatements/parser_continueTarget6.ts` | not supported | expected `;` after `continue`, on ` continue target; ` |
| `parser/ecmascript5/Statements/LabeledStatements/parser_duplicateLabel1.ts` | not supported | expected `;` after expression, on ` target: ` |
| `parser/ecmascript5/Statements/LabeledStatements/parser_duplicateLabel2.ts` | not supported | expected `;` after expression, on ` target: ` |
| `parser/ecmascript5/Statements/LabeledStatements/parser_duplicateLabel3.ts` | not supported | expected `;` after expression, on ` target: ` |
| `parser/ecmascript5/Statements/LabeledStatements/parser_duplicateLabel4.ts` | not supported | expected `;` after expression, on ` target: ` |
| `parser/ecmascript5/Statements/parserDoStatement2.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/Statements/parserES5ForOfStatement10.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/Statements/parserES5ForOfStatement11.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/Statements/parserES5ForOfStatement12.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (const {a, b} of X) { ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement13.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (let {a, b} of X) { ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement14.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/Statements/parserES5ForOfStatement15.ts` | duplicate | of `parser/ecmascript5/Statements/parserES5ForOfStatement14.ts` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement16.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (let {a, b} of X) { ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement17.ts` | not supported | `let` declaration requires an initializer, on ` for (let of; ;) { } ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement18.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448 |
| `parser/ecmascript5/Statements/parserES5ForOfStatement19.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448 |
| `parser/ecmascript5/Statements/parserES5ForOfStatement2.ts` | not supported | `let` declaration requires an initializer, on ` for (let of X) { ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement20.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448 |
| `parser/ecmascript5/Statements/parserES5ForOfStatement21.ts` | not supported | expected expression, on ` for (let of of) { } ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement3.ts` | not supported | `let` declaration requires an initializer, on ` for (let a, b of X) { ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement4.ts` | not supported | expected `;` after declaration, on ` for (let a = 1 of X) { ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement5.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/Statements/parserES5ForOfStatement6.ts` | not supported | expected `;` after declaration, on ` for (let a = 1, b = 2 of X) { ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement7.ts` | not supported | expected `;` after declaration, on ` for (let a: number = 1, b: string = "" of X) { ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement8.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/Statements/parserES5ForOfStatement9.ts` | duplicate | of `parser/ecmascript5/Statements/parserES5ForOfStatement8.ts` |
| `parser/ecmascript5/Statements/parserForInStatement2.ts` | not supported | `in` is a reserved keyword and can't be used as a name, on ` for (let in X) { ` |
| `parser/ecmascript5/Statements/parserForInStatement3.ts` | not supported | `let` declaration requires an initializer, on ` for (let a, b in X) { ` |
| `parser/ecmascript5/Statements/parserForInStatement4.ts` | not supported | expected `;` after declaration, on ` for (let a = 1 in X) { ` |
| `parser/ecmascript5/Statements/parserForInStatement5.ts` | not supported | `let` declaration requires an initializer, on ` for (let a: number in X) { ` |
| `parser/ecmascript5/Statements/parserForInStatement6.ts` | not supported | expected `;` after declaration, on ` for (let a = 1, b = 2 in X) { ` |
| `parser/ecmascript5/Statements/parserForInStatement7.ts` | not supported | expected `;` after declaration, on ` for (let a: number = 1, b: string = "" in X) { ` |
| `parser/ecmascript5/Statements/parserForInStatement8.ts` | not supported | default values inside destructuring patterns are not supported, on ` for (let [x = 'a' in {}] in { '': 0 }) console.log(x) ` |
| `parser/ecmascript5/Statements/parserForStatement2.ts` | the port changes what it checks | `tsc` then reports TS7034, TS7005, TS2538 |
| `parser/ecmascript5/Statements/parserForStatement3.ts` | not supported | invalid assignment target, on ` for(d in _.jh[a]=_.jh[a]\|\|[],b); ` |
| `parser/ecmascript5/Statements/parserForStatement4.ts` | not supported | expected `;` after assignment, on ` for (a = 1 in b) { ` |
| `parser/ecmascript5/Statements/parserForStatement5.ts` | not supported | expected `;` after expression, on ` for ({} in b) { ` |
| `parser/ecmascript5/Statements/parserForStatement6.ts` | not supported | expected `;` after expression, on ` for (foo() in b) { ` |
| `parser/ecmascript5/Statements/parserForStatement7.ts` | not supported | expected `;` after expression, on ` for (new foo() in b) { ` |
| `parser/ecmascript5/Statements/parserForStatement8.ts` | not supported | expected `;` after expression, on ` for (this in b) { ` |
| `parser/ecmascript5/Statements/parserForStatement9.ts` | not supported | default values inside destructuring patterns are not supported, on ` for (let [x = 'a' in {}] = []; !x; x = !x) console.log(x) ` |
| `parser/ecmascript5/Statements/parserIfStatement2.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/Statements/parserWithStatement2.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/Statements/ReturnStatements/parserReturnStatement1.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/Statements/ReturnStatements/parserReturnStatement2.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/Statements/ReturnStatements/parserReturnStatement3.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/Statements/ReturnStatements/parserReturnStatement4.ts` | not supported | expected `,` or `}`, on ` let v = { get foo() { return } }; ` |
| `parser/ecmascript5/StrictMode/octalLiteralInStrictModeES3.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/StrictMode/parserStrictMode1.ts` | checks too little | 4 after the port |
| `parser/ecmascript5/StrictMode/parserStrictMode10.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/StrictMode/parserStrictMode11.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/StrictMode/parserStrictMode12.ts` | the port changes what it checks | `tsc` then reports TS7032, TS7006 |
| `parser/ecmascript5/StrictMode/parserStrictMode13.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/StrictMode/parserStrictMode14.ts` | not supported | expected `;` after expression, on ` with (a) { ` |
| `parser/ecmascript5/StrictMode/parserStrictMode15-negative.ts` | not supported | the `delete` operator is not supported, on ` delete a[b]; ` |
| `parser/ecmascript5/StrictMode/parserStrictMode15.ts` | not supported | the `delete` operator is not supported, on ` delete a; ` |
| `parser/ecmascript5/StrictMode/parserStrictMode16.ts` | not supported | expected `;` after expression, on ` delete 1; ` |
| `parser/ecmascript5/StrictMode/parserStrictMode2.ts` | checks too little | 4 after the port |
| `parser/ecmascript5/StrictMode/parserStrictMode3-negative.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/StrictMode/parserStrictMode3.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/StrictMode/parserStrictMode4.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/StrictMode/parserStrictMode5.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/StrictMode/parserStrictMode6-negative.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/StrictMode/parserStrictMode6.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/StrictMode/parserStrictMode7.ts` | not supported | expected expression, on ` ++eval; ` |
| `parser/ecmascript5/StrictMode/parserStrictMode8.ts` | checks too little | 1 after the port |
| `parser/ecmascript5/StrictMode/parserStrictMode9.ts` | checks too little | 3 after the port |
| `parser/ecmascript5/SuperExpressions/parserSuperExpression1.ts` | not supported | expected `;` after expression, on ` namespace M1.M2 { ` |
| `parser/ecmascript5/SuperExpressions/parserSuperExpression2.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/SuperExpressions/parserSuperExpression3.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/SuperExpressions/parserSuperExpression4.ts` | not supported | expected `;` after expression, on ` namespace M1.M2 { ` |
| `parser/ecmascript5/Symbols/` | not supported | `Symbol` |
| `parser/ecmascript5/TupleTypes/TupleType1.ts` | checks too little | 3 after the port |
| `parser/ecmascript5/TupleTypes/TupleType2.ts` | checks too little | 3 after the port |
| `parser/ecmascript5/TupleTypes/TupleType3.ts` | not supported | tuple types must have at least one element, on ` let v: [] = null as unknown as ([]); ` |
| `parser/ecmascript5/TupleTypes/TupleType4.ts` | the port changes what it checks | `tsc` then reports TS1110, TS2304 |
| `parser/ecmascript5/TupleTypes/TupleType5.ts` | checks too little | 3 after the port |
| `parser/ecmascript5/TupleTypes/TupleType6.ts` | not supported | expected type, on ` let v: [number,,] = null as unknown as ([number,,]); ` |
| `parser/ecmascript5/Types/parserTypeQuery1.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/Types/parserTypeQuery2.ts` | duplicate | of `parser/ecmascript5/Types/parserTypeQuery1.ts` |
| `parser/ecmascript5/Types/parserTypeQuery3.ts` | not supported | expected a property name after `.`, on ` let v: typeof A. = null as unknown as (typeof A.); ` |
| `parser/ecmascript5/Types/parserTypeQuery4.ts` | not supported | expected a property name after `.`, on ` let v: typeof A. = null as unknown as (typeof A.); ` |
| `parser/ecmascript5/Types/parserTypeQuery5.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/Types/parserTypeQuery6.ts` | duplicate | of `parser/ecmascript5/Types/parserTypeQuery5.ts` |
| `parser/ecmascript5/Types/parserTypeQuery7.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/Types/parserTypeQuery8.ts` | not supported | `let` declaration requires an initializer, on ` let v: typeof A<B> = null as unknown as (typeof A<B>); ` |
| `parser/ecmascript5/Types/parserTypeQuery9.ts` | checks too little | 2 after the port |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration1.ts` | not supported | expected `;` after declaration, on ` let selection = a, ` |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration10.ts` | not supported | `let` declaration requires an initializer, on ` let a,; ` |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration11.ts` | not supported | `let` declaration requires an initializer, on ` let a,b; ` |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration3.ts` | not supported | expected expression, on ` , outerr = new Harness.Compiler.WriterAggregator() ` |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration4.ts` | the port changes what it checks | `tsc` then reports TS7005 |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration5.ts` | not supported | `let` declaration requires an initializer, on ` let a, ` |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration6.ts` | the port changes what it checks | `tsc` then reports TS1212, TS2304 |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration7.ts` | not supported | `let` declaration requires an initializer, on ` let a,b ` |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration8.ts` | the port changes what it checks | `tsc` then reports TS1212, TS2304 |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration9.ts` | not supported | `let` declaration requires an initializer, on ` let a; ` |
| `parser/ecmascript6/ComputedPropertyNames/` | not supported | computed property names |
| `parser/ecmascript6/Iterators/parserForOfStatement10.ts` | duplicate | of `parser/ecmascript5/Statements/parserES5ForOfStatement10.ts` |
| `parser/ecmascript6/Iterators/parserForOfStatement11.ts` | duplicate | of `parser/ecmascript5/Statements/parserES5ForOfStatement11.ts` |
| `parser/ecmascript6/Iterators/parserForOfStatement12.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (const {a, b} of X) { ` |
| `parser/ecmascript6/Iterators/parserForOfStatement13.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (let {a, b} of X) { ` |
| `parser/ecmascript6/Iterators/parserForOfStatement14.ts` | duplicate | of `parser/ecmascript5/Statements/parserES5ForOfStatement14.ts` |
| `parser/ecmascript6/Iterators/parserForOfStatement15.ts` | duplicate | of `parser/ecmascript5/Statements/parserES5ForOfStatement14.ts` |
| `parser/ecmascript6/Iterators/parserForOfStatement16.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (let {a, b} of X) { ` |
| `parser/ecmascript6/Iterators/parserForOfStatement17.ts` | not supported | `let` declaration requires an initializer, on ` for (let of; ;) { } ` |
| `parser/ecmascript6/Iterators/parserForOfStatement18.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448 |
| `parser/ecmascript6/Iterators/parserForOfStatement19.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448 |
| `parser/ecmascript6/Iterators/parserForOfStatement2.ts` | not supported | `let` declaration requires an initializer, on ` for (let of X) { ` |
| `parser/ecmascript6/Iterators/parserForOfStatement20.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448 |
| `parser/ecmascript6/Iterators/parserForOfStatement21.ts` | not supported | expected expression, on ` for (let of of) { } ` |
| `parser/ecmascript6/Iterators/parserForOfStatement22.ts` | not supported | `let` declaration requires an initializer, on ` let async; ` |
| `parser/ecmascript6/Iterators/parserForOfStatement23.ts` | not supported | expected `;` after expression, on ` async function foo(x: any): Promise<void> { ` |
| `parser/ecmascript6/Iterators/parserForOfStatement24.ts` | not supported | `let` declaration requires an initializer, on ` let async; ` |
| `parser/ecmascript6/Iterators/parserForOfStatement25.ts` | not supported | default values inside destructuring patterns are not supported, on ` for (let [x = 'a' in {}] of [[]]) console.log(x) ` |
| `parser/ecmascript6/Iterators/parserForOfStatement3.ts` | not supported | `let` declaration requires an initializer, on ` for (let a, b of X) { ` |
| `parser/ecmascript6/Iterators/parserForOfStatement4.ts` | not supported | expected `;` after declaration, on ` for (let a = 1 of X) { ` |
| `parser/ecmascript6/Iterators/parserForOfStatement5.ts` | duplicate | of `parser/ecmascript5/Statements/parserES5ForOfStatement5.ts` |
| `parser/ecmascript6/Iterators/parserForOfStatement6.ts` | not supported | expected `;` after declaration, on ` for (let a = 1, b = 2 of X) { ` |
| `parser/ecmascript6/Iterators/parserForOfStatement7.ts` | not supported | expected `;` after declaration, on ` for (let a: number = 1, b: string = "" of X) { ` |
| `parser/ecmascript6/Iterators/parserForOfStatement8.ts` | duplicate | of `parser/ecmascript5/Statements/parserES5ForOfStatement8.ts` |
| `parser/ecmascript6/Iterators/parserForOfStatement9.ts` | duplicate | of `parser/ecmascript5/Statements/parserES5ForOfStatement8.ts` |
| `parser/ecmascript6/ShorthandPropertyAssignment/parserShorthandPropertyAssignment1.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2322 |
| `parser/ecmascript6/ShorthandPropertyAssignment/parserShorthandPropertyAssignment2.ts` | not supported | expected `:` after field name, on ` let v = { class }; ` |
| `parser/ecmascript6/ShorthandPropertyAssignment/parserShorthandPropertyAssignment3.ts` | not supported | expected `:` after field name, on ` let v = { "" }; ` |
| `parser/ecmascript6/ShorthandPropertyAssignment/parserShorthandPropertyAssignment4.ts` | not supported | expected field name, on ` let v = { 0 }; ` |
| `parser/ecmascript6/ShorthandPropertyAssignment/parserShorthandPropertyAssignment5.ts` | not supported | expected `,` or `}`, on ` let obj = { greet? }; ` |
| `parser/ecmascript6/Symbols/` | not supported | `Symbol` |
| `pedantic/noUncheckedIndexedAccess.ts` | the port changes what it checks | `tsc` then reports TS1335 |
| `references/library-reference-1.ts` | multi-file or JavaScript |  |
| `references/library-reference-10.ts` | multi-file or JavaScript |  |
| `references/library-reference-11.ts` | multi-file or JavaScript |  |
| `references/library-reference-12.ts` | multi-file or JavaScript |  |
| `references/library-reference-13.ts` | multi-file or JavaScript |  |
| `references/library-reference-14.ts` | multi-file or JavaScript |  |
| `references/library-reference-15.ts` | multi-file or JavaScript |  |
| `references/library-reference-2.ts` | multi-file or JavaScript |  |
| `references/library-reference-3.ts` | multi-file or JavaScript |  |
| `references/library-reference-4.ts` | multi-file or JavaScript |  |
| `references/library-reference-5.ts` | multi-file or JavaScript |  |
| `references/library-reference-6.ts` | multi-file or JavaScript |  |
| `references/library-reference-7.ts` | multi-file or JavaScript |  |
| `references/library-reference-8.ts` | multi-file or JavaScript |  |
| `references/library-reference-scoped-packages.ts` | multi-file or JavaScript |  |
| `salsa/annotatedThisPropertyInitializerDoesntNarrow.ts` | multi-file or JavaScript |  |
| `salsa/assignmentToVoidZero1.ts` | multi-file or JavaScript |  |
| `salsa/assignmentToVoidZero2.ts` | multi-file or JavaScript |  |
| `salsa/binderUninitializedModuleExportsAssignment.ts` | multi-file or JavaScript |  |
| `salsa/chainedPrototypeAssignment.ts` | multi-file or JavaScript |  |
| `salsa/checkSpecialPropertyAssignments.ts` | multi-file or JavaScript |  |
| `salsa/circularMultipleAssignmentDeclaration.ts` | multi-file or JavaScript |  |
| `salsa/classCanExtendConstructorFunction.ts` | multi-file or JavaScript |  |
| `salsa/commonJSAliasedExport.ts` | multi-file or JavaScript |  |
| `salsa/commonJSImportClassTypeReference.ts` | multi-file or JavaScript |  |
| `salsa/commonJSImportExportedClassExpression.ts` | multi-file or JavaScript |  |
| `salsa/commonJSImportNestedClassTypeReference.ts` | multi-file or JavaScript |  |
| `salsa/commonJSReexport.ts` | multi-file or JavaScript |  |
| `salsa/conflictingCommonJSES2015Exports.ts` | multi-file or JavaScript |  |
| `salsa/constructorFunctionMergeWithClass.ts` | multi-file or JavaScript |  |
| `salsa/constructorFunctionMethodTypeParameters.ts` | multi-file or JavaScript |  |
| `salsa/constructorFunctions.ts` | multi-file or JavaScript |  |
| `salsa/constructorFunctions2.ts` | multi-file or JavaScript |  |
| `salsa/constructorFunctions3.ts` | multi-file or JavaScript |  |
| `salsa/constructorFunctionsStrict.ts` | multi-file or JavaScript |  |
| `salsa/constructorNameInAccessor.ts` | not supported | parameter requires a type annotation, on ` set constructor(value) {} ` |
| `salsa/constructorNameInGenerator.ts` | not supported | expected class member name, on ` *constructor(): Generator<never, void, unknown> {} ` |
| `salsa/constructorNameInObjectLiteralAccessor.ts` | not supported | expected `,` or `}`, on ` get constructor() { return }, ` |
| `salsa/contextualTypedSpecialAssignment.ts` | multi-file or JavaScript |  |
| `salsa/defaultPropertyAssignedClassWithPrototype.ts` | multi-file or JavaScript |  |
| `salsa/enumMergeWithExpando.ts` | multi-file or JavaScript |  |
| `salsa/expandoOnAlias.ts` | multi-file or JavaScript |  |
| `salsa/exportDefaultInJsFile01.ts` | not supported | JavaScript |
| `salsa/exportDefaultInJsFile02.ts` | not supported | JavaScript |
| `salsa/exportNestedNamespaces.ts` | multi-file or JavaScript |  |
| `salsa/exportNestedNamespaces2.ts` | multi-file or JavaScript |  |
| `salsa/exportPropertyAssignmentNameResolution.ts` | multi-file or JavaScript |  |
| `salsa/globalMergeWithCommonJSAssignmentDeclaration.ts` | multi-file or JavaScript |  |
| `salsa/importAliasModuleExports.ts` | multi-file or JavaScript |  |
| `salsa/importingExportingTypes.ts` | multi-file or JavaScript |  |
| `salsa/inferingFromAny.ts` | multi-file or JavaScript |  |
| `salsa/inferringClassMembersFromAssignments.ts` | multi-file or JavaScript |  |
| `salsa/inferringClassMembersFromAssignments2.ts` | multi-file or JavaScript |  |
| `salsa/inferringClassMembersFromAssignments3.ts` | multi-file or JavaScript |  |
| `salsa/inferringClassMembersFromAssignments4.ts` | multi-file or JavaScript |  |
| `salsa/inferringClassMembersFromAssignments5.ts` | multi-file or JavaScript |  |
| `salsa/inferringClassMembersFromAssignments6.ts` | multi-file or JavaScript |  |
| `salsa/inferringClassMembersFromAssignments7.ts` | multi-file or JavaScript |  |
| `salsa/inferringClassMembersFromAssignments8.ts` | the port changes what it checks | `tsc` then reports TS2683, TS7009 |
| `salsa/inferringClassStaticMembersFromAssignments.ts` | multi-file or JavaScript |  |
| `salsa/jsContainerMergeJsContainer.ts` | multi-file or JavaScript |  |
| `salsa/jsContainerMergeTsDeclaration.ts` | multi-file or JavaScript |  |
| `salsa/jsContainerMergeTsDeclaration2.ts` | multi-file or JavaScript |  |
| `salsa/jsContainerMergeTsDeclaration3.ts` | multi-file or JavaScript |  |
| `salsa/jsdocConstructorFunctionTypeReference.ts` | multi-file or JavaScript |  |
| `salsa/jsObjectsMarkedAsOpenEnded.ts` | multi-file or JavaScript |  |
| `salsa/lateBoundAssignmentDeclarationSupport1.ts` | multi-file or JavaScript |  |
| `salsa/lateBoundAssignmentDeclarationSupport2.ts` | multi-file or JavaScript |  |
| `salsa/lateBoundAssignmentDeclarationSupport3.ts` | multi-file or JavaScript |  |
| `salsa/lateBoundAssignmentDeclarationSupport4.ts` | multi-file or JavaScript |  |
| `salsa/lateBoundAssignmentDeclarationSupport5.ts` | multi-file or JavaScript |  |
| `salsa/lateBoundAssignmentDeclarationSupport6.ts` | multi-file or JavaScript |  |
| `salsa/lateBoundAssignmentDeclarationSupport7.ts` | multi-file or JavaScript |  |
| `salsa/lateBoundClassMemberAssignmentJS.ts` | multi-file or JavaScript |  |
| `salsa/lateBoundClassMemberAssignmentJS2.ts` | multi-file or JavaScript |  |
| `salsa/lateBoundClassMemberAssignmentJS3.ts` | multi-file or JavaScript |  |
| `salsa/malformedTags.ts` | multi-file or JavaScript |  |
| `salsa/methodsReturningThis.ts` | multi-file or JavaScript |  |
| `salsa/mixedPropertyElementAccessAssignmentDeclaration.ts` | the port changes what it checks | `tsc` then reports TS7034, TS7005 |
| `salsa/moduleExportAlias.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportAlias2.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportAlias3.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportAlias4.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportAlias5.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportAliasElementAccessExpression.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportAliasExports.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportAliasImported.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportAliasUnknown.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportAssignment.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportAssignment2.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportAssignment3.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportAssignment4.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportAssignment5.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportAssignment6.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportAssignment7.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportDuplicateAlias.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportDuplicateAlias2.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportDuplicateAlias3.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportNestedNamespaces.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportPropertyAssignmentDefault.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportsAliasLoop1.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportsAliasLoop2.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportWithExportPropertyAssignment.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportWithExportPropertyAssignment2.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportWithExportPropertyAssignment3.ts` | multi-file or JavaScript |  |
| `salsa/moduleExportWithExportPropertyAssignment4.ts` | multi-file or JavaScript |  |
| `salsa/multipleDeclarations.ts` | multi-file or JavaScript |  |
| `salsa/namespaceAssignmentToRequireAlias.ts` | multi-file or JavaScript |  |
| `salsa/nestedDestructuringOfRequire.ts` | multi-file or JavaScript |  |
| `salsa/nestedPrototypeAssignment.ts` | multi-file or JavaScript |  |
| `salsa/plainJSBinderErrors.ts` | multi-file or JavaScript |  |
| `salsa/plainJSGrammarErrors.ts` | multi-file or JavaScript |  |
| `salsa/plainJSGrammarErrors2.ts` | multi-file or JavaScript |  |
| `salsa/plainJSGrammarErrors3.ts` | multi-file or JavaScript |  |
| `salsa/plainJSGrammarErrors4.ts` | multi-file or JavaScript |  |
| `salsa/plainJSRedeclare.ts` | multi-file or JavaScript |  |
| `salsa/plainJSRedeclare2.ts` | multi-file or JavaScript |  |
| `salsa/plainJSRedeclare3.ts` | multi-file or JavaScript |  |
| `salsa/plainJSReservedStrict.ts` | multi-file or JavaScript |  |
| `salsa/plainJSTypeErrors.ts` | multi-file or JavaScript |  |
| `salsa/privateConstructorFunction.ts` | multi-file or JavaScript |  |
| `salsa/privateIdentifierExpando.ts` | multi-file or JavaScript |  |
| `salsa/propertiesOfGenericConstructorFunctions.ts` | multi-file or JavaScript |  |
| `salsa/propertyAssignmentOnImportedSymbol.ts` | multi-file or JavaScript |  |
| `salsa/propertyAssignmentOnParenthesizedNumber.ts` | multi-file or JavaScript |  |
| `salsa/propertyAssignmentOnUnresolvedImportedSymbol.ts` | multi-file or JavaScript |  |
| `salsa/propertyAssignmentUseParentType2.ts` | multi-file or JavaScript |  |
| `salsa/propertyAssignmentUseParentType3.ts` | not supported | `any` is not supported, on ` function foo2(): any[] { ` |
| `salsa/prototypePropertyAssignmentMergeAcrossFiles.ts` | multi-file or JavaScript |  |
| `salsa/prototypePropertyAssignmentMergeAcrossFiles2.ts` | multi-file or JavaScript |  |
| `salsa/prototypePropertyAssignmentMergedTypeReference.ts` | multi-file or JavaScript |  |
| `salsa/prototypePropertyAssignmentMergeWithInterfaceMethod.ts` | multi-file or JavaScript |  |
| `salsa/reExportJsFromTs.ts` | multi-file or JavaScript |  |
| `salsa/requireAssertsFromTypescript.ts` | multi-file or JavaScript |  |
| `salsa/requireOfESWithPropertyAccess.ts` | multi-file or JavaScript |  |
| `salsa/requireTwoPropertyAccesses.ts` | multi-file or JavaScript |  |
| `salsa/sourceFileMergeWithFunction.ts` | multi-file or JavaScript |  |
| `salsa/spellingUncheckedJS.ts` | multi-file or JavaScript |  |
| `salsa/thisPropertyAssignment.ts` | multi-file or JavaScript |  |
| `salsa/thisPropertyAssignmentCircular.ts` | multi-file or JavaScript |  |
| `salsa/thisPropertyAssignmentComputed.ts` | multi-file or JavaScript |  |
| `salsa/thisPropertyAssignmentInherited.ts` | multi-file or JavaScript |  |
| `salsa/thisTypeOfConstructorFunctions.ts` | multi-file or JavaScript |  |
| `salsa/topLevelThisAssignment.ts` | multi-file or JavaScript |  |
| `salsa/typeFromContextualThisType.ts` | multi-file or JavaScript |  |
| `salsa/typeFromJSConstructor.ts` | multi-file or JavaScript |  |
| `salsa/typeFromJSInitializer.ts` | multi-file or JavaScript |  |
| `salsa/typeFromJSInitializer2.ts` | multi-file or JavaScript |  |
| `salsa/typeFromJSInitializer3.ts` | multi-file or JavaScript |  |
| `salsa/typeFromJSInitializer4.ts` | multi-file or JavaScript |  |
| `salsa/typeFromParamTagForFunction.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment10_1.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment10.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment11.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment12.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment13.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment14.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment15.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment16.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment17.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment18.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment19.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment2.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment20.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment21.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment22.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment23.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment24.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment25.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment26.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment27.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment28.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment3.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment32.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment33.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment34.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment35.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment37.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment38.ts` | checks too little | 4 after the port |
| `salsa/typeFromPropertyAssignment39.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment4.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment40.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment5.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment6.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment7.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment8_1.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment8.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment9_1.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignment9.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignmentOutOfOrder.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPropertyAssignmentWithExport.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPrototypeAssignment.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPrototypeAssignment2.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPrototypeAssignment3.ts` | multi-file or JavaScript |  |
| `salsa/typeFromPrototypeAssignment4.ts` | multi-file or JavaScript |  |
| `salsa/typeLookupInIIFE.ts` | multi-file or JavaScript |  |
| `salsa/typeTagOnFunctionReferencesGeneric.ts` | multi-file or JavaScript |  |
| `salsa/unannotatedParametersAreOptional.ts` | multi-file or JavaScript |  |
| `salsa/varRequireFromJavascript.ts` | multi-file or JavaScript |  |
| `salsa/varRequireFromTypescript.ts` | multi-file or JavaScript |  |
| `scanner/ecmascript3/scannerES3NumericLiteral1.ts` | checks too little | 0 after the port |
| `scanner/ecmascript3/scannerES3NumericLiteral2.ts` | checks too little | 1 after the port |
| `scanner/ecmascript3/scannerES3NumericLiteral3.ts` | checks too little | 1 after the port |
| `scanner/ecmascript3/scannerES3NumericLiteral4.ts` | not supported | missing digits in exponent, on ` 1e ` |
| `scanner/ecmascript3/scannerES3NumericLiteral5.ts` | checks too little | 0 after the port |
| `scanner/ecmascript3/scannerES3NumericLiteral6.ts` | not supported | missing digits in exponent, on ` 1e+ ` |
| `scanner/ecmascript3/scannerES3NumericLiteral7.ts` | checks too little | 0 after the port |
| `scanner/ecmascript5/scanner10.1.1-8gs.ts` | duplicate | of `parser/ecmascript5/parser10.1.1-8gs.ts` |
| `scanner/ecmascript5/scannerAdditiveExpression1.ts` | duplicate | of `parser/ecmascript5/parserAdditiveExpression1.ts` |
| `scanner/ecmascript5/scannerClass2.ts` | duplicate | of `parser/ecmascript5/ClassDeclarations/parserClass2.ts` |
| `scanner/ecmascript5/scannerEnum1.ts` | checks too little | 0 after the port |
| `scanner/ecmascript5/scannerImportDeclaration1.ts` | not supported | expected `from` after import specifier list, on ` import TypeScript = TypeScriptServices.TypeScript; ` |
| `scanner/ecmascript5/scannerNonAsciiHorizontalWhitespace.ts` | checks too little | 0 after the port |
| `scanner/ecmascript5/scannerNumericLiteral1.ts` | duplicate | of `scanner/ecmascript3/scannerES3NumericLiteral1.ts` |
| `scanner/ecmascript5/scannerNumericLiteral2.ts` | duplicate | of `scanner/ecmascript3/scannerES3NumericLiteral2.ts` |
| `scanner/ecmascript5/scannerNumericLiteral3.ts` | duplicate | of `scanner/ecmascript3/scannerES3NumericLiteral3.ts` |
| `scanner/ecmascript5/scannerNumericLiteral4.ts` | not supported | missing digits in exponent, on ` 1e ` |
| `scanner/ecmascript5/scannerNumericLiteral5.ts` | duplicate | of `scanner/ecmascript3/scannerES3NumericLiteral5.ts` |
| `scanner/ecmascript5/scannerNumericLiteral6.ts` | not supported | missing digits in exponent, on ` 1e+ ` |
| `scanner/ecmascript5/scannerNumericLiteral7.ts` | duplicate | of `scanner/ecmascript3/scannerES3NumericLiteral7.ts` |
| `scanner/ecmascript5/scannerNumericLiteral8.ts` | checks too little | 1 after the port |
| `scanner/ecmascript5/scannerNumericLiteral9.ts` | checks too little | 1 after the port |
| `scanner/ecmascript5/scannerS7.2_A1.5_T2.ts` | the port changes what it checks | `tsc` then reports TS2448 |
| `scanner/ecmascript5/scannerS7.3_A1.1_T2.ts` | not supported | expected identifier after `let`/`const`, on ` x ` |
| `scanner/ecmascript5/scannerS7.4_A2_T2.ts` | porter failure | nothing to prune at offsets 332; our first unsupported error: unterminated block comment |
| `scanner/ecmascript5/scannerS7.6_A4.2_T1.ts` | not supported | unexpected character `\`, on ` let \u0410 = 1; ` |
| `scanner/ecmascript5/scannerS7.8.3_A6.1_T1.ts` | not supported | missing digits after `0x`, on ` 0x ` |
| `scanner/ecmascript5/scannerS7.8.4_A7.1_T4.ts` | not supported | invalid unicode escape: expected 4 hex digits, on ` "\u000G" ` |
| `scanner/ecmascript5/scannerStringLiterals.ts` | not supported | unknown escape sequence `\ `, on ` '\u2192\   ' ` |
| `scanner/ecmascript5/scannerStringLiteralWithContainingNullCharacter1.ts` | checks too little | 0 after the port |
| `scanner/ecmascript5/scannerUnexpectedNullCharacter1.ts` | porter failure | nothing to prune at offsets 22, 24; our first unsupported error: unexpected byte 0x00 |
| `scanner/ecmascript5/scannerUnicodeEscapeInKeyword1.ts` | checks too little | 1 after the port |
| `scanner/ecmascript5/scannerUnicodeEscapeInKeyword2.ts` | multi-file or JavaScript |  |
| `scanner/jsdocInvalidTokens.ts` | multi-file or JavaScript |  |
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
| `statements/throwStatements/invalidThrowStatement.ts` | porter failure | still pruning after 40 passes; our first unsupported error: expected expression after `throw` |
| `statements/throwStatements/throwInEnclosingStatements.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `statements/throwStatements/throwStatements.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `statements/tryStatements/catchClauseWithTypeAnnotation.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `statements/tryStatements/invalidTryStatements.ts` | not supported | expected expression, on ` catch(x) { } // error missing try ` |
| `statements/tryStatements/tryStatements.ts` | the port changes what it checks | `tsc` then reports TS2492 |
| `statements/VariableStatements/everyTypeWithAnnotationAndInitializer.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `statements/VariableStatements/everyTypeWithInitializer.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `statements/VariableStatements/invalidMultipleVariableDeclarations.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `statements/VariableStatements/recursiveInitializer.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448, TS2454, TS7023 |
| `statements/VariableStatements/usingDeclarations/` | not supported | `using` declarations |
| `statements/withStatements/` | not supported | `with` |
| `Symbols/` | not supported | `Symbol` |
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
| `types/literal/booleanLiteralTypes1.ts` | the port changes what it checks | `tsc` then reports TS2393, TS2345 |
| `types/literal/booleanLiteralTypes2.ts` | the port changes what it checks | `tsc` then reports TS2393, TS2345 |
| `types/literal/enumLiteralTypes1.ts` | the port changes what it checks | `tsc` then reports TS2393, TS2345 |
| `types/literal/enumLiteralTypes2.ts` | the port changes what it checks | `tsc` then reports TS2393, TS2345 |
| `types/literal/enumLiteralTypes3.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum Choice { Unknown, Yes, No }; ` |
| `types/literal/literalTypesWidenInParameterPosition.ts` | checks too little | 4 after the port |
| `types/literal/numericLiteralTypes1.ts` | the port changes what it checks | `tsc` then reports TS2393, TS2345 |
| `types/literal/numericLiteralTypes2.ts` | the port changes what it checks | `tsc` then reports TS2393, TS2345 |
| `types/literal/numericStringLiteralTypes.ts` | not supported | unexpected character `&`, on `` type T0 = string & `${string}`;  // string `` |
| `types/literal/stringEnumLiteralTypes1.ts` | the port changes what it checks | `tsc` then reports TS2393, TS2345 |
| `types/literal/stringEnumLiteralTypes2.ts` | the port changes what it checks | `tsc` then reports TS2393, TS2345 |
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
| `types/members/objectTypeWithCallSignatureAppearsToBeFunctionType.ts` | not supported | `any` is not supported, on ` let r2b: (x: any, y?: any) => any = i.apply; ` |
| `types/members/objectTypeWithCallSignatureHidingMembersOfExtendedFunction.ts` | the port changes what it checks | `tsc` then reports TS7053 |
| `types/members/objectTypeWithCallSignatureHidingMembersOfFunction.ts` | not supported | `any` is not supported, on ` apply(a: any, b?: any): void; ` |
| `types/members/objectTypeWithCallSignatureHidingMembersOfFunctionAssignmentCompat.ts` | not supported | expected field name in object type, on ` (): void ` |
| `types/members/objectTypeWithConstructSignatureAppearsToBeFunctionType.ts` | not supported | construct signatures |
| `types/members/objectTypeWithConstructSignatureHidingMembersOfExtendedFunction.ts` | not supported | construct signatures |
| `types/members/objectTypeWithConstructSignatureHidingMembersOfFunction.ts` | not supported | construct signatures |
| `types/members/objectTypeWithConstructSignatureHidingMembersOfFunctionAssignmentCompat.ts` | not supported | construct signatures |
| `types/members/objectTypeWithDuplicateNumericProperty.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `types/members/objectTypeWithNumericProperty.ts` | not supported | expected class member name, on ` 1: number; ` |
| `types/members/objectTypeWithStringAndNumberIndexSignatureToAny.ts` | not supported | index signatures |
| `types/members/objectTypeWithStringIndexerHidingObjectIndexer.ts` | not supported | index signatures |
| `types/members/objectTypeWithStringNamedNumericProperty.ts` | the port changes what it checks | `tsc` then reports TS2448, TS2454 |
| `types/members/objectTypeWithStringNamedPropertyOfIllegalCharacters.ts` | the port changes what it checks | `tsc` then reports TS2551 |
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
| `types/rest/objectRest.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
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
| `types/specifyingTypes/predefinedTypes/objectTypesWithPredefinedTypesAsName2.ts` | porter failure | still pruning after 40 passes; our first unsupported error: `void` is a reserved keyword and can't be used as a name |
| `types/specifyingTypes/typeLiterals/arrayOfFunctionTypes3.ts` | not supported | expected `(` after constructor name in `new` expression, on ` let r3 = new y[0](); ` |
| `types/specifyingTypes/typeLiterals/arrayTypeOfFunctionTypes.ts` | the port changes what it checks | `tsc` then reports TS7053, TS7009 |
| `types/specifyingTypes/typeLiterals/arrayTypeOfFunctionTypes2.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/specifyingTypes/typeLiterals/arrayTypeOfTypeOf.ts` | not supported | `let` declaration requires an initializer, on ` let xs3: typeof Array<number> = null as unknown as (typeof Array<number>); ` |
| `types/specifyingTypes/typeLiterals/functionLiteral.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/specifyingTypes/typeLiterals/functionLiteralForOverloads.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/specifyingTypes/typeLiterals/functionLiteralForOverloads2.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/specifyingTypes/typeLiterals/parenthesizedTypes.ts` | the port changes what it checks | `tsc` then reports TS7051 |
| `types/specifyingTypes/typeLiterals/unionTypeLiterals.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/specifyingTypes/typeQueries/invalidTypeOfTarget.ts` | not supported | expected a value name after `typeof`, on ` let x1: typeof = null as unknown as (typeof) {}; ` |
| `types/specifyingTypes/typeQueries/typeofClass2.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7010 |
| `types/specifyingTypes/typeQueries/typeofClassWithPrivates.ts` | not supported | `as` to `C<string>` is not yet supported: class types aren't yet supported as `as` targets, on ` let c: C<string> = null as unknown as (C<string>); ` |
| `types/specifyingTypes/typeQueries/typeofModuleWithoutExports.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `types/specifyingTypes/typeQueries/typeofThis.ts` | the port changes what it checks | `tsc` then reports TS18047 |
| `types/specifyingTypes/typeQueries/typeofThisWithImplicitThis.ts` | the port changes what it checks | `tsc` then reports TS2683 |
| `types/specifyingTypes/typeQueries/typeofTypeParameter.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `types/specifyingTypes/typeQueries/typeQueryOnClass.ts` | the port changes what it checks | `tsc` then reports TS7006, TS7010 |
| `types/specifyingTypes/typeQueries/typeQueryWithReservedWords.ts` | not supported | `let` is a reserved keyword and can't be used as a name, on ` let: typeof Controller.prototype.let;        // Should not error ` |
| `types/specifyingTypes/typeReferences/genericTypeReferenceWithoutTypeArgument3.ts` | not supported | expected `;` after expression, on ` declare class C<T> { ` |
| `types/spread/objectSpreadComputedProperty.ts` | not supported | `any` is not supported, on ` let a: any = null; ` |
| `types/spread/objectSpreadNegativeParse.ts` | the port changes what it checks | `tsc` then reports TS2554 |
| `types/spread/objectSpreadNoTransform.ts` | not supported | `let` declaration requires an initializer, on ` let b; ` |
| `types/spread/objectSpreadSetonlyAccessor.ts` | the port changes what it checks | `tsc` then reports TS2322 |
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
| `types/tuple/castingTuple.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
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
| `types/tuple/strictTupleLength.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
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
| `types/typeAliases/reservedNamesInAliases.ts` | porter failure | nothing to prune at offsets 110, 110; our first unsupported error: expected `;` after expression |
| `types/typeAliases/typeAliases.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeAliases/typeAliasesForObjectTypes.ts` | checks too little | 2 after the port |
| `types/typeParameters/recurringTypeParamForContainerOfBase01.ts` | not supported | expected `,` or `>`, on ` interface BoxOfFoo<T extends Foo<T>> { ` |
| `types/typeParameters/typeArgumentLists/callNonGenericFunctionWithTypeArguments.ts` | the port changes what it checks | `tsc` then reports TS7010, TS2722, TS18048 |
| `types/typeParameters/typeArgumentLists/constraintSatisfactionWithAny.ts` | not supported | expected `,` or `>`, on ` function foo<T extends String>(x: T): T { return null; } ` |
| `types/typeParameters/typeArgumentLists/constraintSatisfactionWithAny2.ts` | not supported | expected `,` or `>`, on ` function foo<Z, T extends <U>(x: U) => Z>(y: T): Z { return null as unknown a... ` |
| `types/typeParameters/typeArgumentLists/constraintSatisfactionWithEmptyObject.ts` | not supported | expected `,` or `>`, on ` function foo<T extends Object>(x: T): void { } ` |
| `types/typeParameters/typeArgumentLists/functionConstraintSatisfaction.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeParameters/typeArgumentLists/functionConstraintSatisfaction2.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeParameters/typeArgumentLists/functionConstraintSatisfaction3.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeParameters/typeArgumentLists/instantiateGenericClassWithZeroTypeArguments.ts` | checks too little | 4 after the port |
| `types/typeParameters/typeArgumentLists/instantiateNonGenericTypeWithTypeArguments.ts` | the port changes what it checks | `tsc` then reports TS7009 |
| `types/typeParameters/typeArgumentLists/instantiationExpressionErrors.ts` | not supported | expected field name in object type, on ` let f: { <T>(): T, g<U>(): U } = null as unknown as ({ <T>(): T, g<U>(): U }); ` |
| `types/typeParameters/typeArgumentLists/instantiationExpressions.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeParameters/typeArgumentLists/typeParameterAsTypeParameterConstraint.ts` | not supported | expected `,` or `>`, on ` function foo<T, U extends T>(x: T, y: U): U { return y; } ` |
| `types/typeParameters/typeArgumentLists/typeParameterAsTypeParameterConstraint2.ts` | not supported | expected `,` or `>`, on ` function foo<T, U extends T>(x: T, y: U): U { return y; } // this is now an e... ` |
| `types/typeParameters/typeArgumentLists/typeParameterAsTypeParameterConstraintTransitively.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/typeParameters/typeArgumentLists/typeParameterAsTypeParameterConstraintTransitively2.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/typeParameters/typeArgumentLists/wrappedAndRecursiveConstraints.ts` | not supported | expected `,` or `>`, on ` class C<T extends Date> { ` |
| `types/typeParameters/typeArgumentLists/wrappedAndRecursiveConstraints2.ts` | not supported | expected `,` or `>`, on ` class C<T extends C<T>> { // error ` |
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
| `types/typeParameters/typeParameterLists/typeParametersAvailableInNestedScope.ts` | not supported | expected type, on ` x: <U>(a: U) => T = <U>(a: U) => { ` |
| `types/typeParameters/typeParameterLists/typeParametersAvailableInNestedScope2.ts` | checks too little | 2 after the port |
| `types/typeParameters/typeParameterLists/typeParametersAvailableInNestedScope3.ts` | not supported | expected type, on ` function foo<T>(v: T): { a: <T>(a: T) => T; b: () => T; c: <T>(v: T) => { a: ... ` |
| `types/typeParameters/typeParameterLists/typeParameterUsedAsConstraint.ts` | not supported | expected `,` or `>`, on ` class C<T, U extends T> { } ` |
| `types/typeParameters/typeParameterLists/varianceAnnotations.ts` | not supported | expected `,` or `>`, on ` type Covariant<out T> = { ` |
| `types/typeParameters/typeParameterLists/varianceAnnotationsWithCircularlyReferencesError.ts` | not supported | `in` is a reserved keyword and can't be used as a name, on ` type T1<in in> = T1 // Error: circularly references ` |
| `types/typeRelationships/apparentType/apparentTypeSubtyping.ts` | not supported | expected `,` or `>`, on ` class Base<U extends String> { ` |
| `types/typeRelationships/apparentType/apparentTypeSupertype.ts` | not supported | expected `,` or `>`, on ` class Derived<U extends String> extends Base { // error ` |
| `types/typeRelationships/assignmentCompatibility/anyAssignabilityInInheritance.ts` | the port changes what it checks | `tsc` then reports TS2393, TS7006 |
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
| `types/typeRelationships/assignmentCompatibility/enumAssignabilityInInheritance.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
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
| `types/typeRelationships/subtypesAndSuperTypes/nullIsSubtypeOfEverythingButUndefined.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `types/typeRelationships/subtypesAndSuperTypes/stringLiteralTypeIsSubtypeOfString.ts` | the port changes what it checks | `tsc` then reports TS7010, TS2322 |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfAny.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameter.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameterWithConstraints.ts` | not supported | expected `,` or `>`, on ` class D1<T extends U, U> extends C3<T> { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameterWithConstraints2.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameterWithConstraints3.ts` | not supported | expected `,` or `>`, on ` function f<T extends U, U, V>(t: T, u: U, v: V): void { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameterWithConstraints4.ts` | not supported | expected `,` or `>`, on ` function f<T extends Foo, U extends Foo, V>(t: T, u: U, v: V): void { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameterWithRecursiveConstraints.ts` | not supported | expected `,` or `>`, on ` function f<T extends Foo<U>, U extends Foo<T>, V extends Foo<V>>(t: T, u: U, ... ` |
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
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithPrivates2.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithPrivates3.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` interface T2 { z } ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithPublics.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithStringIndexers.ts` | not supported | index signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithStringIndexers2.ts` | not supported | index signatures |
| `types/typeRelationships/typeAndMemberIdentity/primtiveTypesAreIdentical.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/typeParametersAreIdenticalToThemselves.ts` | the port changes what it checks | `tsc` then reports TS7010 |
| `types/typeRelationships/typeAndMemberIdentity/unionTypeIdentity.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `types/typeRelationships/typeInference/bivariantInferences.ts` | not supported | `this` is a reserved keyword and can't be used as a name, on ` equalsShallow<T>(this: ReadonlyArray<T>, other: ReadonlyArray<T>): boolean; ` |
| `types/typeRelationships/typeInference/contextualSignatureInstantiation.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `types/typeRelationships/typeInference/discriminatedUnionInference.ts` | not supported | expected field name in object type, on ` type Foo<A> = { type: "foo", (): A[] }; ` |
| `types/typeRelationships/typeInference/genericCallToOverloadedMethodWithOverloadedArguments.ts` | the port changes what it checks | `tsc` then reports TS2393 |
| `types/typeRelationships/typeInference/genericCallTypeArgumentInference.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeRelationships/typeInference/genericCallWithConstraintsTypeArgumentInference.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeRelationships/typeInference/genericCallWithConstraintsTypeArgumentInference2.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `types/typeRelationships/typeInference/genericCallWithConstructorTypedArguments5.ts` | not supported | expected type, on ` function foo<T, U>(arg: { cb: new(t: T) => U }): U { ` |
| `types/typeRelationships/typeInference/genericCallWithFunctionTypedArguments2.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeRelationships/typeInference/genericCallWithFunctionTypedArguments3.ts` | not supported | expected field name in object type, on ` (x: boolean): boolean; ` |
| `types/typeRelationships/typeInference/genericCallWithFunctionTypedArguments4.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeRelationships/typeInference/genericCallWithGenericSignatureArguments.ts` | the port changes what it checks | `tsc` then reports TS2345 |
| `types/typeRelationships/typeInference/genericCallWithGenericSignatureArguments2.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `types/typeRelationships/typeInference/genericCallWithGenericSignatureArguments3.ts` | the port changes what it checks | `tsc` then reports TS1263, TS2322 |
| `types/typeRelationships/typeInference/genericCallWithNonSymmetricSubtypes.ts` | not supported | `as` to `T` is not yet supported: generic type parameters are erased at runtime, on ` let r: T = null as unknown as (T); ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgs.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgs2.ts` | not supported | expected `,` or `>`, on ` function f<T extends Base, U extends Base>(a: { x: T; y: U }): (T \| U)[] { ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndConstraints.ts` | not supported | expected `,` or `>`, on ` function foo<T extends { x: string }>(t: X<T>, t2: X<T>): T { ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndConstraints2.ts` | not supported | expected `,` or `>`, on ` function f<T extends Base>(x: { foo: T; bar: T }): T { ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndConstraints3.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndConstraints4.ts` | not supported | expected `,` or `>`, on ` function foo<T, U extends T>(t: T, t2: U): (x: T) => U { ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndConstraints5.ts` | not supported | expected `,` or `>`, on ` function foo<T, U extends T>(t: T, t2: U): (x: T) => U { ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndIndexers.ts` | not supported | index signatures |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndIndexersErrors.ts` | not supported | index signatures |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndNumericIndexer.ts` | not supported | index signatures |
| `types/typeRelationships/typeInference/genericCallWithOverloadedConstructorTypedArguments.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeRelationships/typeInference/genericCallWithOverloadedConstructorTypedArguments2.ts` | not supported | construct signatures (SUB-1026: read as a method named `new`) |
| `types/typeRelationships/typeInference/genericCallWithOverloadedFunctionTypedArguments.ts` | the port changes what it checks | `tsc` then reports TS2345, TS2322 |
| `types/typeRelationships/typeInference/genericCallWithOverloadedFunctionTypedArguments2.ts` | the port changes what it checks | `tsc` then reports TS1263 |
| `types/typeRelationships/typeInference/genericClassWithFunctionTypedMemberArguments.ts` | not supported | expected `;` after expression, on ` namespace ImmediatelyFix { ` |
| `types/typeRelationships/typeInference/genericClassWithObjectTypeArgsAndConstraints.ts` | not supported | expected `;` after expression, on ` namespace Class { ` |
| `types/typeRelationships/typeInference/genericContextualTypes1.ts` | not supported | expected type, on ` const f00: <A>(x: A) => A[] = list; ` |
| `types/typeRelationships/typeInference/genericContextualTypes2.ts` | not supported | unexpected character `&`, on ` type LowInfer<T> = T & {}; ` |
| `types/typeRelationships/typeInference/genericContextualTypes3.ts` | not supported | unexpected character `&`, on ` type LowInfer<T> = T & {}; ` |
| `types/typeRelationships/typeInference/genericFunctionParameters.ts` | not supported | expected type, on ` function f1<T>(cb: <S>(x: S) => T): T { return null as unknown as (T); } ` |
| `types/typeRelationships/typeInference/indexSignatureTypeInference.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `types/typeRelationships/typeInference/keyofInferenceIntersectsResults.ts` | not supported | expected `,` or `>`, on ` function foo<T = X>(x: keyof T, y: keyof T): T { return null as unknown as (T... ` |
| `types/typeRelationships/typeInference/keyofInferenceLowerPriorityThanReturn.ts` | not supported | unexpected character `&`, on ` function insertOnConflictDoNothing<Req extends object, Def extends object>(_t... ` |
| `types/typeRelationships/typeInference/noInfer.ts` | not supported | unexpected character `&`, on `` type T05 = NoInfer<`foo${string}` & `${string}bar`>; `` |
| `types/typeRelationships/typeInference/noInferRedeclaration.ts` | multi-file or JavaScript |  |
| `types/typeRelationships/typeInference/unionAndIntersectionInference1.ts` | not supported | intersection types |
| `types/typeRelationships/typeInference/unionAndIntersectionInference2.ts` | not supported | intersection types |
| `types/typeRelationships/typeInference/unionAndIntersectionInference3.ts` | not supported | intersection types |
| `types/typeRelationships/typeInference/unionTypeInference.ts` | not supported | unexpected character `&`, on ` function f4<T>(x: string & T): T { return null as unknown as (T); } ` |
| `types/typeRelationships/widenedTypes/initializersWidened.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/typeRelationships/widenedTypes/strictNullChecksNoWidening.ts` | not supported | expected expression, on ` let a3 = void 0; ` |
| `types/union/contextualTypeWithUnionTypeCallSignatures.ts` | not supported | duplicate call signature on interface `IWithCallSignatures4`, on ` (a: string, b: number): number; ` |
| `types/union/contextualTypeWithUnionTypeIndexSignatures.ts` | not supported | index signatures |
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
| `types/union/unionTypeEquivalence.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `types/union/unionTypeIndexSignature.ts` | not supported | index signatures |
| `types/union/unionTypePropertyAccessibility.ts` | not supported | `protected` is not supported, on ` protected member: string; ` |
| `types/union/unionTypeReduction.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `types/union/unionTypeReduction2.ts` | not supported | optional function parameters are not yet supported, on ` function f1(x: { f(): void }, y: { f(x?: string): void }): void { ` |
| `types/union/unionTypeWithIndexSignature.ts` | not supported | index signatures |
| `types/uniqueSymbol/` | not supported | `Symbol` |
| `types/unknown/unknownType2.ts` | the port changes what it checks | `tsc` then reports TS1335 |
| `types/witness/witness.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `typings/typingsLookup1.ts` | multi-file or JavaScript |  |
| `typings/typingsLookup2.ts` | multi-file or JavaScript |  |
| `typings/typingsLookup3.ts` | multi-file or JavaScript |  |
| `typings/typingsLookup4.ts` | multi-file or JavaScript |  |
| `typings/typingsLookupAmd.ts` | multi-file or JavaScript |  |
| `typings/typingsSuggestion1.ts` | multi-file or JavaScript |  |
| `typings/typingsSuggestion2.ts` | multi-file or JavaScript |  |
| `typings/typingsSuggestionBun1.ts` | multi-file or JavaScript |  |
| `typings/typingsSuggestionBun2.ts` | multi-file or JavaScript |  |
