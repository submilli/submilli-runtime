# Upstream cases left out

Every upstream conformance case that isn't in the suite, and why. A directory (ending
in `/`) is left out whole, with its subdirectories, except for any case in the suite.
Written by `../typescript-baselines/port-suite.cjs`: change its lists or the porter,
not this file. Declaration files (`.d.ts`) aren't cases, and `.tsx` files aren't read.

3482 entries: 3211 not supported, 73 the port changes what it checks, 120 duplicate, 77 checks too little, 1 porter failure.

| Case | Reason | Detail |
|:-----|:-------|:-------|
| `additionalChecks/noPropertyAccessFromIndexSignature1.ts` | not supported | index signatures |
| `ambient/ambientDeclarations.ts` | not supported | `let` declaration requires an initializer, on ` let n; ` |
| `ambient/ambientDeclarationsExternal.ts` | not supported | script files sharing one global scope |
| `ambient/ambientDeclarationsPatterns_merging1.ts` | not supported | expected `;` after expression, on ` declare module "*.foo" { ` |
| `ambient/ambientDeclarationsPatterns_merging2.ts` | not supported | expected `;` after expression, on ` declare module "*.foo" { ` |
| `ambient/ambientDeclarationsPatterns_merging3.ts` | not supported | script files sharing one global scope |
| `ambient/ambientDeclarationsPatterns_tooManyAsterisks.ts` | not supported | expected `;` after expression, on ` declare module "too*many*asterisks" { } ` |
| `ambient/ambientDeclarationsPatterns.ts` | not supported | declaration files (`.d.ts`) |
| `ambient/ambientEnumDeclaration1.ts` | not supported | expected `;` after expression, on ` declare enum E { ` |
| `ambient/ambientEnumDeclaration2.ts` | not supported | expected `;` after expression, on ` declare enum E { ` |
| `ambient/ambientErrors.ts` | not supported | expected expression, on ` function fn4(): void { }: void { }; ` |
| `ambient/ambientExternalModuleInsideNonAmbient.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `ambient/ambientExternalModuleInsideNonAmbientExternalModule.ts` | not supported | expected a declaration after `export`, on ` export declare module "M" { } ` |
| `ambient/ambientExternalModuleMerging.ts` | not supported | script files sharing one global scope |
| `ambient/ambientInsideNonAmbient.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `ambient/ambientInsideNonAmbientExternalModule.ts` | not supported | `let` declaration requires an initializer, on ` export let x; ` |
| `ambient/ambientModuleDeclarationWithReservedIdentifierInDottedPath.ts` | not supported | expected `;` after expression, on ` declare namespace chrome.debugger { ` |
| `ambient/ambientModuleDeclarationWithReservedIdentifierInDottedPath2.ts` | not supported | expected `;` after expression, on ` declare namespace chrome.debugger { ` |
| `ambient/ambientShorthand_declarationEmit.ts` | not supported | expected `;` after expression, on ` declare module "foo"; ` |
| `ambient/ambientShorthand_duplicate.ts` | not supported | declaration files (`.d.ts`) |
| `ambient/ambientShorthand_merging.ts` | not supported | declaration files (`.d.ts`) |
| `ambient/ambientShorthand_reExport.ts` | not supported | declaration files (`.d.ts`) |
| `ambient/ambientShorthand.ts` | not supported | declaration files (`.d.ts`) |
| `async/` | not supported | async/await |
| `asyncGenerators/` | not supported | async/await and generators |
| `classes/awaitAndYieldInProperty.ts` | not supported | expected `;` after expression, on ` async function* test(x: Promise<string>): AsyncGenerator<1 \| 3 \| 2 \| 4, typeo... ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractAccessor.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractAsIdentifier.ts` | not supported | expected `(` after constructor name in `new` expression, on ` new abstract; ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractAssignabilityConstructorFunction.ts` | not supported | expected `;` after expression, on ` abstract class A { } ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractClinterfaceAssignability.ts` | not supported | construct signatures |
| `classes/classDeclarations/classAbstractKeyword/classAbstractConstructor.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractConstructorAssignability.ts` | not supported | expected `;` after expression, on ` abstract class B extends A {} ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractCrashedOnce.ts` | not supported | expected `;` after expression, on ` abstract class foo { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractExtends.ts` | not supported | expected `;` after expression, on ` abstract class B extends A { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractFactoryFunction.ts` | not supported | expected `;` after expression, on ` abstract class B extends A {} ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractGeneric.ts` | not supported | expected `;` after expression, on ` abstract class A<T> { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractImportInstantiation.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractInAModule.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractInheritance1.ts` | not supported | expected `;` after expression, on ` abstract class A {} ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractInheritance2.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractInstantiations1.ts` | not supported | expected `;` after expression, on ` abstract class A {} ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractInstantiations2.ts` | not supported | expected `;` after expression, on ` abstract class B { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractManyKeywords.ts` | not supported | `export default` is not supported, on ` export default abstract class A {} ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractMergedDeclaration.ts` | not supported | expected `;` after expression, on ` abstract class CM {} ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractMethodInNonAbstractClass.ts` | not supported | abstract classes are not supported, on ` abstract foo(); ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractMethodWithImplementation.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractMixedWithModifiers.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractOverloads.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractOverrideWithAbstract.ts` | not supported | expected `;` after expression, on ` abstract class B extends A { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractProperties.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractSingleLineDecl.ts` | not supported | expected `;` after expression, on ` abstract class A {} ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractSuperCalls.ts` | not supported | expected `;` after expression, on ` abstract class B extends A { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractUsingAbstractMethod1.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractUsingAbstractMethods2.ts` | not supported | abstract classes are not supported, on ` abstract foo(); ` |
| `classes/classDeclarations/classAbstractKeyword/classAbstractWithInterface.ts` | not supported | expected `;` after expression, on ` abstract interface I {} ` |
| `classes/classDeclarations/classAndInterfaceMergeConflictingMembers.ts` | not supported | expected `;` after expression, on ` declare class C1 { ` |
| `classes/classDeclarations/classAndInterfaceWithSameName.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `classes/classDeclarations/classAndVariableWithSameName.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `classes/classDeclarations/classBody/classBodyWithStatements.ts` | not supported | expected `:` and a type for the class field, on ` let x = 1; ` |
| `classes/classDeclarations/classBody/classWithEmptyBody.ts` | not supported | `as` to `C` is not yet supported: class types aren't yet supported as `as` targets, on ` let c: C = null as unknown as (C); ` |
| `classes/classDeclarations/classDeclarationLoop.ts` | not supported | expected expression, on ` for (let i = 0; i < 10; ++i) { ` |
| `classes/classDeclarations/classExtendingBuiltinType.ts` | not supported | unknown type `Function`, on ` class C2 extends Function { } ` |
| `classes/classDeclarations/classExtendingClassLikeType.ts` | not supported | construct signatures |
| `classes/classDeclarations/classExtendingNull.ts` | checks too little | 0 after the port |
| `classes/classDeclarations/classHeritageSpecification/classAppearsToHaveMembersOfObject.ts` | not supported | `as` to `C` is not yet supported: class types aren't yet supported as `as` targets, on ` let c: C = null as unknown as (C); ` |
| `classes/classDeclarations/classHeritageSpecification/classExtendingOptionalChain.ts` | not supported | expected `;` after expression, on ` namespace A { ` |
| `classes/classDeclarations/classHeritageSpecification/classExtendsEveryObjectType2.ts` | not supported | tuple types must have at least one element, on ` class C6 extends []{ } // error ` |
| `classes/classDeclarations/classHeritageSpecification/classExtendsItselfIndirectly2.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `classes/classDeclarations/classHeritageSpecification/classExtendsItselfIndirectly3.ts` | not supported | script files sharing one global scope |
| `classes/classDeclarations/classHeritageSpecification/classExtendsShadowedConstructorFunction.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `classes/classDeclarations/classHeritageSpecification/classExtendsValidConstructorFunction.ts` | the port changes what it checks | `tsc` then reports TS7009 |
| `classes/classDeclarations/classHeritageSpecification/constructorFunctionTypeIsAssignableToBaseType.ts` | not supported | `any` is not supported, on ` bar: any; ` |
| `classes/classDeclarations/classHeritageSpecification/constructorFunctionTypeIsAssignableToBaseType2.ts` | not supported | `any` is not supported, on ` constructor(x: any) { ` |
| `classes/classDeclarations/classHeritageSpecification/derivedTypeDoesNotRequireExtendsClause.ts` | not supported | `as` to `Base` is not yet supported: class types aren't yet supported as `as` targets, on ` let b: Base = null as unknown as (Base); ` |
| `classes/classDeclarations/classImplementsMergedClassInterface.ts` | not supported | expected `;` after expression, on ` declare class C1 { ` |
| `classes/classDeclarations/classInsideBlock.ts` | not supported | a `class` must be declared at the top level of the module, on ` class C { } ` |
| `classes/classDeclarations/classWithPredefinedTypesAsNames2.ts` | not supported | `void` is a reserved keyword and can't be used as a name, on ` class void {} ` |
| `classes/classDeclarations/classWithSemicolonClassElement1.ts` | checks too little | 0 after the port |
| `classes/classDeclarations/classWithSemicolonClassElement2.ts` | duplicate | of `classes/classDeclarations/classWithSemicolonClassElement1.ts` |
| `classes/classDeclarations/declaredClassMergedwithSelf.ts` | not supported | script files sharing one global scope |
| `classes/classDeclarations/mergeClassInterfaceAndModule.ts` | not supported | expected `;` after expression, on ` declare class C1 {} ` |
| `classes/classDeclarations/mergedClassInterface.ts` | not supported | script files sharing one global scope |
| `classes/classDeclarations/modifierOnClassDeclarationMemberInFunction.ts` | not supported | a `class` must be declared at the top level of the module, on ` class C { ` |
| `classes/classExpression.ts` | not supported | expected expression, on ` let x = class C { ` |
| `classes/classExpressions/classExpression1.ts` | not supported | expected expression, on ` let v = class C {}; ` |
| `classes/classExpressions/classExpression2.ts` | not supported | expected expression, on ` let v = class C extends D {}; ` |
| `classes/classExpressions/classExpression3.ts` | not supported | expected expression, on ` let C = class extends class extends class { a: number = 1 } { b: number = 2 }... ` |
| `classes/classExpressions/classExpression4.ts` | not supported | expected expression, on ` let C = class { ` |
| `classes/classExpressions/classExpression5.ts` | not supported | expected expression, on ` new class { ` |
| `classes/classExpressions/classExpressionLoop.ts` | not supported | expected expression, on ` for (let i = 0; i < 10; ++i) { ` |
| `classes/classExpressions/classWithStaticFieldInParameterBindingPattern.2.ts` | not supported | expected field name in object pattern, on ` (({ [class extends C { static x: number = 1 }.x]: b = "" }) => { let C; })(); ` |
| `classes/classExpressions/classWithStaticFieldInParameterBindingPattern.3.ts` | not supported | expected field name in object pattern, on ` (({ [class extends C { static x: number = 1 }.x]: b = "" }) => { let C; })(); ` |
| `classes/classExpressions/classWithStaticFieldInParameterBindingPattern.ts` | not supported | expected field name in object pattern, on ` (({ [class { static x: number = 1 }.x]: b = "" }) => {})(); ` |
| `classes/classExpressions/classWithStaticFieldInParameterInitializer.2.ts` | not supported | expected expression, on ` ((b = class extends C { static x: number = 1 }) => { let C; })(); ` |
| `classes/classExpressions/classWithStaticFieldInParameterInitializer.3.ts` | not supported | expected expression, on ` ((b = class extends C { static x: number = 1 }) => { let C; })(); ` |
| `classes/classExpressions/classWithStaticFieldInParameterInitializer.ts` | not supported | expected expression, on ` ((b = class { static x: number = 1 }) => {})(); ` |
| `classes/classExpressions/extendClassExpressionFromModule.ts` | not supported | expected a declaration after `export`, on ` export = x; ` |
| `classes/classExpressions/genericClassExpressionInFunction.ts` | not supported | expected a value name after `typeof`, on ` function B1<U>(): typeof (Anonymous class) { ` |
| `classes/classExpressions/modifierOnClassExpressionMemberInFunction.ts` | not supported | expected expression, on ` let x = class C { ` |
| `classes/classStaticBlock/` | not supported | static blocks |
| `classes/constructorDeclarations/automaticConstructors/derivedClassWithoutExplicitConstructor2.ts` | not supported | expected `{`, on ` constructor(x: number, y?: number, z?: number); ` |
| `classes/constructorDeclarations/classConstructorAccessibility.ts` | not supported | `protected` is not supported, on ` protected constructor(public x: number) { } ` |
| `classes/constructorDeclarations/classConstructorAccessibility3.ts` | not supported | `protected` is not supported, on ` protected constructor(public x: number) { } ` |
| `classes/constructorDeclarations/classConstructorAccessibility4.ts` | not supported | a `class` must be declared at the top level of the module, on ` class B { ` |
| `classes/constructorDeclarations/classConstructorAccessibility5.ts` | not supported | `protected` is not supported, on ` protected constructor() { } ` |
| `classes/constructorDeclarations/classConstructorOverloadsAccessibility.ts` | not supported | expected `{`, on ` protected constructor(a: number) // error ` |
| `classes/constructorDeclarations/classConstructorParametersAccessibility.ts` | not supported | `protected` is a reserved word in strict mode and can't be used as a name, on ` constructor(protected p: number) { } ` |
| `classes/constructorDeclarations/classConstructorParametersAccessibility2.ts` | not supported | `protected` is a reserved word in strict mode and can't be used as a name, on ` constructor(protected p?: number) { } ` |
| `classes/constructorDeclarations/classConstructorParametersAccessibility3.ts` | not supported | `protected` is a reserved word in strict mode and can't be used as a name, on ` constructor(protected p: number) { } ` |
| `classes/constructorDeclarations/classWithTwoConstructorDefinitions.ts` | not supported | parameter requires a type annotation, on ` constructor(x) { } // error ` |
| `classes/constructorDeclarations/constructorParameters/constructorImplementationWithDefaultValues.ts` | not supported | parameter requires a type annotation, on ` constructor(x); ` |
| `classes/constructorDeclarations/constructorParameters/constructorImplementationWithDefaultValues2.ts` | not supported | parameter requires a type annotation, on ` constructor(x); ` |
| `classes/constructorDeclarations/constructorParameters/constructorOverloadsWithDefaultValues.ts` | not supported | expected `{`, on ` constructor(x: number = 1); // error ` |
| `classes/constructorDeclarations/constructorParameters/constructorOverloadsWithOptionalParameters.ts` | not supported | parameter requires a type annotation, on ` constructor(x?, y?: any[]); ` |
| `classes/constructorDeclarations/constructorParameters/constructorParameterProperties.ts` | not supported | `protected` is a reserved word in strict mode and can't be used as a name, on ` constructor(private x: string, protected z: string) { } ` |
| `classes/constructorDeclarations/constructorParameters/constructorParameterProperties2.ts` | not supported | `protected` is a reserved word in strict mode and can't be used as a name, on ` constructor(protected y: number) { } // error ` |
| `classes/constructorDeclarations/constructorParameters/declarationEmitReadonly.ts` | checks too little | 0 after the port |
| `classes/constructorDeclarations/constructorParameters/readonlyInAmbientClass.ts` | not supported | expected `;` after expression, on ` declare class C{ ` |
| `classes/constructorDeclarations/constructorWithExpressionLessReturn.ts` | checks too little | 0 after the port |
| `classes/constructorDeclarations/quotedConstructors.ts` | not supported | unknown escape sequence `\x`, on ` "\x63onstructor"() { ` |
| `classes/constructorDeclarations/superCalls/derivedClassSuperCallsInNonConstructorMembers.ts` | not supported | expected type, on ` a: super(); ` |
| `classes/constructorDeclarations/superCalls/derivedClassSuperCallsWithThisArg.ts` | not supported | parameter requires a type annotation, on ` constructor(a) { } ` |
| `classes/constructorDeclarations/superCalls/derivedClassSuperProperties.ts` | not supported | unexpected character `@`, on ` @decorate(this) ` |
| `classes/constructorDeclarations/superCalls/emitStatementsBeforeSuperCallWithDefineFields.ts` | duplicate | of `classes/constructorDeclarations/superCalls/emitStatementsBeforeSuperCall.ts` |
| `classes/indexMemberDeclarations/` | not supported | index signatures |
| `classes/members/accessibility/classPropertyAsPrivate.ts` | not supported | parameter requires a type annotation, on ` private set y(x) { } ` |
| `classes/members/accessibility/classPropertyAsProtected.ts` | not supported | `protected` is not supported, on ` protected x: string; ` |
| `classes/members/accessibility/classPropertyIsPublicByDefault.ts` | not supported | parameter requires a type annotation, on ` set y(x) { } ` |
| `classes/members/accessibility/privateClassPropertyAccessibleWithinClass.ts` | not supported | parameter requires a type annotation, on ` private set y(x) { this.y = this.x; } ` |
| `classes/members/accessibility/privateClassPropertyAccessibleWithinNestedClass.ts` | not supported | parameter requires a type annotation, on ` private set y(x) { this.y = this.x; } ` |
| `classes/members/accessibility/privateInstanceMemberAccessibility.ts` | not supported | `super` is a reserved keyword and can't be used as a name, on ` z: typeof super.foo; // error ` |
| `classes/members/accessibility/privateProtectedMembersAreNotAccessibleDestructuring.ts` | not supported | expected `:` and a type for the class field, on ` private priv; ` |
| `classes/members/accessibility/privateStaticNotAccessibleInClodule.ts` | not supported | expected `;` after expression, on ` namespace C { ` |
| `classes/members/accessibility/privateStaticNotAccessibleInClodule2.ts` | not supported | expected `;` after expression, on ` namespace D { ` |
| `classes/members/accessibility/protectedClassPropertyAccessibleWithinClass.ts` | not supported | `protected` is not supported, on ` protected x: string; ` |
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
| `classes/members/classTypes/genericSetterInClassTypeJsDoc.ts` | not supported | JavaScript |
| `classes/members/classTypes/indexersInClassType.ts` | not supported | expected class member name, on ` [x: number]: Date; ` |
| `classes/members/classTypes/instancePropertiesInheritedIntoClassType.ts` | not supported | expected `;` after expression, on ` namespace NonGeneric { ` |
| `classes/members/classTypes/instancePropertyInClassType.ts` | not supported | expected `;` after expression, on ` namespace NonGeneric { ` |
| `classes/members/classTypes/staticPropertyNotInClassType.ts` | not supported | expected `;` after expression, on ` namespace NonGeneric { ` |
| `classes/members/constructorFunctionTypes/classWithConstructors.ts` | not supported | expected `;` after expression, on ` namespace NonGeneric { ` |
| `classes/members/constructorFunctionTypes/classWithStaticMembers.ts` | not supported | static accessors are not supported, on ` static get x(): number { return 1; } ` |
| `classes/members/constructorFunctionTypes/constructorHasPrototypeProperty.ts` | not supported | expected `;` after expression, on ` namespace NonGeneric { ` |
| `classes/members/inheritanceAndOverriding/derivedClassFunctionOverridesBaseClassAccessor.ts` | not supported | parameter requires a type annotation, on ` set x(v) { ` |
| `classes/members/inheritanceAndOverriding/derivedClassIncludesInheritedMembers.ts` | not supported | parameter requires a type annotation, on ` set c(v) { } ` |
| `classes/members/inheritanceAndOverriding/derivedClassOverridesIndexersWithAssignmentCompatibility.ts` | not supported | index signatures |
| `classes/members/inheritanceAndOverriding/derivedClassOverridesProtectedMembers.ts` | not supported | `protected` is not supported, on ` protected a: typeof x; ` |
| `classes/members/inheritanceAndOverriding/derivedClassOverridesProtectedMembers2.ts` | not supported | `protected` is not supported, on ` protected a: typeof x; ` |
| `classes/members/inheritanceAndOverriding/derivedClassOverridesProtectedMembers3.ts` | not supported | static accessors are not supported, on ` static get t(): { foo: string; } { return x; } ` |
| `classes/members/inheritanceAndOverriding/derivedClassOverridesProtectedMembers4.ts` | not supported | `protected` is not supported, on ` protected a: typeof x; ` |
| `classes/members/inheritanceAndOverriding/derivedClassOverridesPublicMembers.ts` | not supported | static accessors are not supported, on ` static get t(): { foo: string; } { return x; } ` |
| `classes/members/inheritanceAndOverriding/derivedClassOverridesWithoutSubtype.ts` | not supported | `any` is not supported, on ` foo: any; ` |
| `classes/members/inheritanceAndOverriding/derivedClassTransitivity.ts` | not supported | `as` to `C` is not yet supported: class types aren't yet supported as `as` targets, on ` let c: C = null as unknown as (C); ` |
| `classes/members/inheritanceAndOverriding/derivedClassTransitivity2.ts` | not supported | `as` to `C` is not yet supported: class types aren't yet supported as `as` targets, on ` let c: C = null as unknown as (C); ` |
| `classes/members/inheritanceAndOverriding/derivedClassTransitivity3.ts` | not supported | `as` to `C<string>` is not yet supported: class types aren't yet supported as `as` targets, on ` let c: C<string> = null as unknown as (C<string>); ` |
| `classes/members/inheritanceAndOverriding/derivedClassTransitivity4.ts` | not supported | `protected` is not supported, on ` protected foo(x: number): void { } ` |
| `classes/members/inheritanceAndOverriding/derivedClassWithAny.ts` | not supported | static accessors are not supported, on ` static get Y(): number { ` |
| `classes/members/inheritanceAndOverriding/derivedClassWithPrivateInstanceShadowingProtectedInstance.ts` | not supported | `protected` is not supported, on ` protected x: string; ` |
| `classes/members/inheritanceAndOverriding/derivedClassWithPrivateInstanceShadowingPublicInstance.ts` | not supported | parameter requires a type annotation, on ` public set a(v) { } ` |
| `classes/members/inheritanceAndOverriding/derivedClassWithPrivateStaticShadowingProtectedStatic.ts` | not supported | `protected` is not supported, on ` protected static x: string; ` |
| `classes/members/inheritanceAndOverriding/derivedClassWithPrivateStaticShadowingPublicStatic.ts` | not supported | static accessors are not supported, on ` public static get a(): number { return 1; } ` |
| `classes/members/inheritanceAndOverriding/derivedGenericClassWithAny.ts` | not supported | expected `,` or `>`, on ` class C<T extends number> { ` |
| `classes/members/instanceAndStaticMembers/superInStaticMembers1.ts` | not supported | expected a declaration after `export`, on ` export declare namespace Bar { type _ = unknown; } ` |
| `classes/members/instanceAndStaticMembers/thisAndSuperInStaticMembers1.ts` | not supported | expected `;` after expression, on ` declare class B { ` |
| `classes/members/instanceAndStaticMembers/thisAndSuperInStaticMembers2.ts` | not supported | expected `;` after expression, on ` declare class B { ` |
| `classes/members/instanceAndStaticMembers/thisAndSuperInStaticMembers3.ts` | not supported | expected `;` after expression, on ` declare class B { ` |
| `classes/members/instanceAndStaticMembers/thisAndSuperInStaticMembers4.ts` | not supported | expected `;` after expression, on ` declare class B { ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInInstanceMember.ts` | not supported | class fields require a type annotation, on ` x = this; ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInInstanceMember2.ts` | not supported | class fields require a type annotation, on ` x = this; ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInstanceMemberNarrowedWithLoopAntecedent.ts` | not supported | expected `:` and a type for the class field, on ` state!: State; ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers10.ts` | not supported | unexpected character `@`, on ` @foo ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers11.ts` | not supported | unexpected character `@`, on ` @foo ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers12.ts` | not supported | expected expression, on ` static bar: typeof Inner =  class Inner { ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers13.ts` | not supported | expected expression, on ` static bar: typeof Inner =  class Inner { ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers2.ts` | not supported | `C` is a class, not a value, so `typeof` has no type to take, on ` static foo: typeof C = this; // ok ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers4.ts` | duplicate | of `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers3.ts` |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers8.ts` | not supported | class fields require a type annotation, on ` static functionExprBoundary = function () { return this.f + 2 }; ` |
| `classes/members/instanceAndStaticMembers/typeOfThisInStaticMembers9.ts` | not supported | class fields require a type annotation, on ` static functionExprBoundary = function () { return super.f + 2 }; ` |
| `classes/members/privateNames/privateNameAccessors.ts` | not supported | unexpected character `#`, on ` get #prop(): string { return ""; } ` |
| `classes/members/privateNames/privateNameAccessorsAccess.ts` | not supported | unexpected character `#`, on ` get #prop(): string { return ""; } ` |
| `classes/members/privateNames/privateNameAccessorsCallExpression.ts` | not supported | unexpected character `#`, on ` get #fieldFunc(): () => void {  return function() { this.x = 10; } } ` |
| `classes/members/privateNames/privateNameAccessorssDerivedClasses.ts` | not supported | unexpected character `#`, on ` get #prop(): number { return  123; } ` |
| `classes/members/privateNames/privateNameAmbientNoImplicitAny.ts` | not supported | unexpected character `#`, on ` #prop; ` |
| `classes/members/privateNames/privateNameAndAny.ts` | not supported | unexpected character `#`, on ` #foo = true; ` |
| `classes/members/privateNames/privateNameAndIndexSignature.ts` | not supported | index signatures |
| `classes/members/privateNames/privateNameAndObjectRestSpread.ts` | not supported | unexpected character `#`, on ` #prop = 1; ` |
| `classes/members/privateNames/privateNameAndPropertySignature.ts` | not supported | unexpected character `#`, on ` #foo: string; ` |
| `classes/members/privateNames/privateNameAndStaticInitializer.ts` | not supported | unexpected character `#`, on ` #foo = 1; ` |
| `classes/members/privateNames/privateNameBadAssignment.ts` | not supported | unexpected character `#`, on ` exports.#nope = 1;           // Error (outside class body) ` |
| `classes/members/privateNames/privateNameBadDeclaration.ts` | not supported | unexpected character `#`, on ` #x: 1,         // Error ` |
| `classes/members/privateNames/privateNameBadSuper.ts` | not supported | unexpected character `#`, on ` #x; ` |
| `classes/members/privateNames/privateNameBadSuperUseDefineForClassFields.ts` | not supported | unexpected character `#`, on ` #x; ` |
| `classes/members/privateNames/privateNameCircularReference.ts` | not supported | unexpected character `#`, on ` #foo = this.#bar; ` |
| `classes/members/privateNames/privateNameClassExpressionLoop.ts` | not supported | unexpected character `#`, on ` #myField = "hello"; ` |
| `classes/members/privateNames/privateNameComputedPropertyName1.ts` | not supported | unexpected character `#`, on ` #a = 'a'; ` |
| `classes/members/privateNames/privateNameComputedPropertyName2.ts` | not supported | unexpected character `#`, on ` #x = 100; ` |
| `classes/members/privateNames/privateNameComputedPropertyName3.ts` | not supported | unexpected character `#`, on ` #name; ` |
| `classes/members/privateNames/privateNameComputedPropertyName4.ts` | not supported | unexpected character `#`, on ` static #qux = 42; ` |
| `classes/members/privateNames/privateNameConstructorReserved.ts` | not supported | unexpected character `#`, on `` #constructor(): void {}      // Error: `#constructor` is a reserved word. `` |
| `classes/members/privateNames/privateNameConstructorSignature.ts` | not supported | construct signatures |
| `classes/members/privateNames/privateNameDeclaration.ts` | not supported | unexpected character `#`, on ` #foo: string; ` |
| `classes/members/privateNames/privateNameDeclarationMerging.ts` | not supported | construct signatures |
| `classes/members/privateNames/privateNameDuplicateField.ts` | not supported | unexpected character `#`, on ` #foo = "foo"; ` |
| `classes/members/privateNames/privateNameEmitHelpers.ts` | not supported | `node_modules` packages |
| `classes/members/privateNames/privateNameEnum.ts` | not supported | unexpected character `#`, on ` #x ` |
| `classes/members/privateNames/privateNameES5Ban.ts` | not supported | unexpected character `#`, on ` #field = 123; ` |
| `classes/members/privateNames/privateNameField.ts` | not supported | unexpected character `#`, on ` #name: string; ` |
| `classes/members/privateNames/privateNameFieldAccess.ts` | not supported | unexpected character `#`, on ` #myField = "hello world"; ` |
| `classes/members/privateNames/privateNameFieldCallExpression.ts` | not supported | unexpected character `#`, on ` #fieldFunc = function() { this.x = 10; }; ` |
| `classes/members/privateNames/privateNameFieldClassExpression.ts` | not supported | unexpected character `#`, on ` #foo = class { ` |
| `classes/members/privateNames/privateNameFieldDerivedClasses.ts` | not supported | unexpected character `#`, on ` #prop: number = 123; ` |
| `classes/members/privateNames/privateNameFieldDestructuredBinding.ts` | not supported | unexpected character `#`, on ` #field = 1; ` |
| `classes/members/privateNames/privateNameFieldInitializer.ts` | not supported | unexpected character `#`, on ` #field = 10; ` |
| `classes/members/privateNames/privateNameFieldParenthesisLeftAssignment.ts` | not supported | unexpected character `#`, on ` #p: number; ` |
| `classes/members/privateNames/privateNameFieldsESNext.ts` | not supported | unexpected character `#`, on ` #a = 10; ` |
| `classes/members/privateNames/privateNameFieldUnaryMutation.ts` | not supported | unexpected character `#`, on ` #test: number = 24; ` |
| `classes/members/privateNames/privateNameHashCharName.ts` | not supported | unexpected character `#`, on ` this.# ` |
| `classes/members/privateNames/privateNameImplicitDeclaration.ts` | not supported | JavaScript |
| `classes/members/privateNames/privateNameInInExpression.ts` | not supported | unexpected character `#`, on ` #field = 1; ` |
| `classes/members/privateNames/privateNameInInExpressionTransform.ts` | not supported | unexpected character `#`, on ` #field = 1; ` |
| `classes/members/privateNames/privateNameInInExpressionUnused.ts` | not supported | unexpected character `#`, on ` #unused: undefined; // expect unused error ` |
| `classes/members/privateNames/privateNameInLhsReceiverExpression.ts` | not supported | unexpected character `#`, on ` #y = 123; ` |
| `classes/members/privateNames/privateNameInObjectLiteral-1.ts` | not supported | unexpected character `#`, on ` #foo: 1 ` |
| `classes/members/privateNames/privateNameInObjectLiteral-2.ts` | not supported | unexpected character `#`, on ` #foo() { ` |
| `classes/members/privateNames/privateNameInObjectLiteral-3.ts` | not supported | unexpected character `#`, on ` get #foo() { ` |
| `classes/members/privateNames/privateNameJsBadAssignment.ts` | not supported | JavaScript |
| `classes/members/privateNames/privateNameJsBadDeclaration.ts` | not supported | JavaScript |
| `classes/members/privateNames/privateNameLateSuper.ts` | not supported | unexpected character `#`, on ` #x; ` |
| `classes/members/privateNames/privateNameLateSuperUseDefineForClassFields.ts` | not supported | unexpected character `#`, on ` #x; ` |
| `classes/members/privateNames/privateNameMethod.ts` | not supported | unexpected character `#`, on ` #method(param: string): string { ` |
| `classes/members/privateNames/privateNameMethodAccess.ts` | not supported | unexpected character `#`, on ` #method(): string { return "" } ` |
| `classes/members/privateNames/privateNameMethodAssignment.ts` | not supported | unexpected character `#`, on ` #method(): void { }; ` |
| `classes/members/privateNames/privateNameMethodAsync.ts` | not supported | unexpected character `#`, on ` async #bar(): Promise<number> { return await Promise.resolve(42); } ` |
| `classes/members/privateNames/privateNameMethodCallExpression.ts` | not supported | unexpected character `#`, on ` #method(): void { this.x = 10; }; ` |
| `classes/members/privateNames/privateNameMethodClassExpression.ts` | not supported | unexpected character `#`, on ` #field = this.#method(); ` |
| `classes/members/privateNames/privateNameMethodInStaticFieldInit.ts` | not supported | unexpected character `#`, on ` static s: number = new C().#method(); ` |
| `classes/members/privateNames/privateNameMethodsDerivedClasses.ts` | not supported | unexpected character `#`, on ` #prop(): number{ return  123; } ` |
| `classes/members/privateNames/privateNameNestedClassAccessorsShadowing.ts` | not supported | unexpected character `#`, on ` get #x(): number { return 1; }; ` |
| `classes/members/privateNames/privateNameNestedClassFieldShadowing.ts` | not supported | unexpected character `#`, on ` #x; ` |
| `classes/members/privateNames/privateNameNestedClassMethodShadowing.ts` | not supported | unexpected character `#`, on ` #x(): void { }; ` |
| `classes/members/privateNames/privateNameNestedClassNameConflict.ts` | not supported | unexpected character `#`, on ` #foo: string; ` |
| `classes/members/privateNames/privateNameNestedMethodAccess.ts` | not supported | unexpected character `#`, on ` #foo = 42; ` |
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
| `classes/members/privateNames/privateNameSetterNoGetter.ts` | not supported | unexpected character `#`, on ` set #x(x) {} ` |
| `classes/members/privateNames/privateNamesIncompatibleModifiers.ts` | not supported | unexpected character `#`, on ` public #foo = 3;         // Error ` |
| `classes/members/privateNames/privateNamesIncompatibleModifiersJs.ts` | not supported | JavaScript |
| `classes/members/privateNames/privateNamesInGenericClasses.ts` | not supported | unexpected character `#`, on ` #foo: T; ` |
| `classes/members/privateNames/privateNamesInNestedClasses-1.ts` | not supported | unexpected character `#`, on ` #foo = "A's #foo"; ` |
| `classes/members/privateNames/privateNamesInNestedClasses-2.ts` | not supported | unexpected character `#`, on ` static #x = 5; ` |
| `classes/members/privateNames/privateNamesInterfaceExtendingClass.ts` | not supported | unexpected character `#`, on ` #prop; ` |
| `classes/members/privateNames/privateNamesNoDelete.ts` | not supported | `delete` |
| `classes/members/privateNames/privateNamesNotAllowedAsParameters.ts` | not supported | unexpected character `#`, on ` setFoo(#foo: string): void {} ` |
| `classes/members/privateNames/privateNamesNotAllowedInVariableDeclarations.ts` | not supported | unexpected character `#`, on ` const #foo = 3; ` |
| `classes/members/privateNames/privateNameStaticAccessors.ts` | not supported | unexpected character `#`, on ` static get #prop(): string { return ""; } ` |
| `classes/members/privateNames/privateNameStaticAccessorsAccess.ts` | not supported | unexpected character `#`, on ` static get #prop(): string { return ""; } ` |
| `classes/members/privateNames/privateNameStaticAccessorsCallExpression.ts` | not supported | unexpected character `#`, on ` static get #fieldFunc(): () => void {  return function() { A.#x = 10; } } ` |
| `classes/members/privateNames/privateNameStaticAccessorssDerivedClasses.ts` | not supported | unexpected character `#`, on ` static get #prop(): number { return  123; } ` |
| `classes/members/privateNames/privateNameStaticAndStaticInitializer.ts` | not supported | unexpected character `#`, on ` static #foo = 1; ` |
| `classes/members/privateNames/privateNameStaticEmitHelpers.ts` | not supported | `node_modules` packages |
| `classes/members/privateNames/privateNameStaticFieldAccess.ts` | not supported | unexpected character `#`, on ` static #myField = "hello world"; ` |
| `classes/members/privateNames/privateNameStaticFieldAssignment.ts` | not supported | unexpected character `#`, on ` static #field = 0; ` |
| `classes/members/privateNames/privateNameStaticFieldCallExpression.ts` | not supported | unexpected character `#`, on ` static #fieldFunc = function () { this.x = 10; }; ` |
| `classes/members/privateNames/privateNameStaticFieldClassExpression.ts` | not supported | unexpected character `#`, on ` static #foo = class { ` |
| `classes/members/privateNames/privateNameStaticFieldDerivedClasses.ts` | not supported | unexpected character `#`, on ` static #prop: number = 123; ` |
| `classes/members/privateNames/privateNameStaticFieldDestructuredBinding.ts` | not supported | unexpected character `#`, on ` static #field = 1; ` |
| `classes/members/privateNames/privateNameStaticFieldInitializer.ts` | not supported | unexpected character `#`, on ` static #field = 10; ` |
| `classes/members/privateNames/privateNameStaticFieldNoInitializer.ts` | not supported | unexpected character `#`, on ` static #x; ` |
| `classes/members/privateNames/privateNameStaticFieldUnaryMutation.ts` | not supported | unexpected character `#`, on ` static #test: number = 24; ` |
| `classes/members/privateNames/privateNameStaticMethod.ts` | not supported | unexpected character `#`, on ` static #method(param: string): string { ` |
| `classes/members/privateNames/privateNameStaticMethodAssignment.ts` | not supported | unexpected character `#`, on ` static #method(): void { }; ` |
| `classes/members/privateNames/privateNameStaticMethodAsync.ts` | not supported | unexpected character `#`, on ` static async #bar(): Promise<number> { return await Promise.resolve(42); } ` |
| `classes/members/privateNames/privateNameStaticMethodCallExpression.ts` | not supported | unexpected character `#`, on ` static #method(): void { this.x = 10; }; ` |
| `classes/members/privateNames/privateNameStaticMethodClassExpression.ts` | not supported | unexpected character `#`, on ` static #field = D.#method(); ` |
| `classes/members/privateNames/privateNameStaticMethodInStaticFieldInit.ts` | not supported | unexpected character `#`, on ` static s: number = C.#method(); ` |
| `classes/members/privateNames/privateNameStaticsAndStaticMethods.ts` | not supported | unexpected character `#`, on ` static #foo(a: number): void {} ` |
| `classes/members/privateNames/privateNamesUnique-1.ts` | not supported | unexpected character `#`, on ` #foo: number; ` |
| `classes/members/privateNames/privateNamesUnique-2.ts` | not supported | unexpected character `#`, on ` #x; ` |
| `classes/members/privateNames/privateNamesUnique-3.ts` | not supported | unexpected character `#`, on ` #foo = 1; ` |
| `classes/members/privateNames/privateNamesUnique-4.ts` | not supported | unexpected character `#`, on ` class C { #something: number } ` |
| `classes/members/privateNames/privateNamesUnique-5.ts` | not supported | unexpected character `#`, on ` #foo: number; ` |
| `classes/members/privateNames/privateNamesUseBeforeDef.ts` | not supported | unexpected character `#`, on ` #foo = this.#bar; // Error ` |
| `classes/members/privateNames/privateNameUncheckedJsOptionalChain.ts` | not supported | JavaScript |
| `classes/members/privateNames/privateNameUnused.ts` | not supported | unexpected character `#`, on ` #used = "used"; ` |
| `classes/members/privateNames/privateNameWhenNotUseDefineForClassFieldsInEsNext.ts` | not supported | unexpected character `#`, on ` #prop = 0 ` |
| `classes/members/privateNames/privateStaticNameShadowing.ts` | not supported | unexpected character `#`, on ` static #f = X.#m(); ` |
| `classes/members/privateNames/privateWriteOnlyAccessorRead.ts` | not supported | unexpected character `#`, on ` set #value(v: { foo: { bar: number } }) {} ` |
| `classes/members/privateNames/typeFromPrivatePropertyAssignment.ts` | not supported | unexpected character `#`, on ` #a?: Foo; ` |
| `classes/members/privateNames/typeFromPrivatePropertyAssignmentJs.ts` | not supported | JavaScript |
| `classes/methodDeclarations/optionalMethodDeclarations.ts` | checks too little | 0 after the port |
| `classes/mixinAbstractClasses.2.ts` | not supported | expected `,` or `>`, on ` function Mixin<TBaseClass extends abstract new (...args: any) => any>(baseCla... ` |
| `classes/mixinAbstractClasses.ts` | not supported | expected `,` or `>`, on ` function Mixin<TBaseClass extends abstract new (...args: any) => any>(baseCla... ` |
| `classes/mixinAbstractClassesReturnTypeInference.ts` | not supported | expected `;` after expression, on ` abstract class AbstractBase { ` |
| `classes/mixinAccessModifiers.ts` | not supported | expected type, on ` type Constructable = new (...args: any[]) => object; ` |
| `classes/mixinAccessors1.ts` | not supported | construct signatures |
| `classes/mixinAccessors2.ts` | not supported | construct signatures |
| `classes/mixinAccessors3.ts` | not supported | construct signatures |
| `classes/mixinAccessors4.ts` | not supported | `any` is not supported, on ` constructor(...args: any[]) {} ` |
| `classes/mixinAccessors5.ts` | not supported | expected `,` or `>`, on ` function basicMixin<T extends object, U extends object>( ` |
| `classes/mixinClassesAnnotated.ts` | not supported | expected type, on ` type Constructor<T> = new(...args: any[]) => T; ` |
| `classes/mixinClassesAnonymous.ts` | not supported | expected type, on ` type Constructor<T> = new(...args: any[]) => T; ` |
| `classes/mixinClassesMembers.ts` | not supported | expected `;` after expression, on ` declare class C1 { ` |
| `classes/mixinWithBaseDependingOnSelfNoCrash1.ts` | not supported | expected `;` after expression, on ` declare class Document<Parent> {} ` |
| `classes/nestedClassDeclaration.ts` | not supported | expected `:` and a type for the class field, on ` class C2 { ` |
| `classes/propertyMemberDeclarations/abstractProperty.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/propertyMemberDeclarations/abstractPropertyInitializer.ts` | not supported | expected `;` after expression, on ` abstract class C { ` |
| `classes/propertyMemberDeclarations/accessibilityModifiers.ts` | not supported | expected `:` and a type for the class field, on ` private static privateProperty; ` |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty.ts` | not supported | parameter requires a type annotation, on ` set p(value) { this._secret = value } // error ` |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty2.ts` | not supported | parameter requires a type annotation, on `` set x(value) { console.log(`x was set to ${value}`); } `` |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty3.ts` | not supported | expected `;` after expression, on ` declare class Animal { ` |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty4.ts` | not supported | expected `;` after expression, on ` declare class Animal { ` |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty5.ts` | not supported | parameter requires a type annotation, on ` set p(value) { } ` |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty6.ts` | duplicate | of `classes/propertyMemberDeclarations/accessorsOverrideProperty.ts` |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty7.ts` | not supported | expected `;` after expression, on ` abstract class A { ` |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty8.ts` | not supported | construct signatures |
| `classes/propertyMemberDeclarations/accessorsOverrideProperty9.ts` | not supported | expected `,` or `>`, on ` export type Constructor<T = {}> = new (...args: any[]) => T; ` |
| `classes/propertyMemberDeclarations/assignParameterPropertyToPropertyDeclarationESNext.ts` | duplicate | of `classes/propertyMemberDeclarations/assignParameterPropertyToPropertyDeclarationES2022.ts` |
| `classes/propertyMemberDeclarations/autoAccessor1.ts` | not supported | expected `:` and a type for the class field, on ` accessor a: any; ` |
| `classes/propertyMemberDeclarations/autoAccessor10.ts` | not supported | unexpected character `#`, on ` #a1_accessor_storage = 1; ` |
| `classes/propertyMemberDeclarations/autoAccessor11.ts` | not supported | expected `:` and a type for the class field, on ` a ` |
| `classes/propertyMemberDeclarations/autoAccessor2.ts` | not supported | unexpected character `#`, on ` accessor #a: any; ` |
| `classes/propertyMemberDeclarations/autoAccessor3.ts` | not supported | expected `:` and a type for the class field, on ` accessor "w": any; ` |
| `classes/propertyMemberDeclarations/autoAccessor4.ts` | not supported | expected `:` and a type for the class field, on ` accessor 0: any; ` |
| `classes/propertyMemberDeclarations/autoAccessor5.ts` | not supported | expected `:` and a type for the class field, on ` accessor ["w"]: any; ` |
| `classes/propertyMemberDeclarations/autoAccessor6.ts` | not supported | expected `:` and a type for the class field, on ` accessor a: any; ` |
| `classes/propertyMemberDeclarations/autoAccessor7.ts` | not supported | expected `;` after expression, on ` abstract class C1 { ` |
| `classes/propertyMemberDeclarations/autoAccessor8.ts` | not supported | expected `:` and a type for the class field, on ` accessor a: any; ` |
| `classes/propertyMemberDeclarations/autoAccessor9.ts` | not supported | unexpected character `#`, on ` #x = 1; ` |
| `classes/propertyMemberDeclarations/autoAccessorAllowedModifiers.ts` | not supported | unexpected character `#`, on ` accessor #j: any; ` |
| `classes/propertyMemberDeclarations/autoAccessorDisallowedModifiers.ts` | not supported | expected `;` after expression, on ` abstract class C1 { ` |
| `classes/propertyMemberDeclarations/autoAccessorExperimentalDecorators.ts` | not supported | unexpected character `@`, on ` @dec ` |
| `classes/propertyMemberDeclarations/autoAccessorNoUseDefineForClassFields.ts` | not supported | two files with one name: `file3` |
| `classes/propertyMemberDeclarations/canFollowGetSetKeyword.ts` | not supported | expected `:` and a type for the class field, on ` *x(): Generator<never, void, unknown> {} ` |
| `classes/propertyMemberDeclarations/constructorParameterShadowsOuterScopes.ts` | not supported | class fields require a type annotation, on ` b = x; // error, evaluated in scope of constructor, cannot reference x ` |
| `classes/propertyMemberDeclarations/constructorParameterShadowsOuterScopes2.ts` | not supported | class fields require a type annotation, on ` b = x; // ok ` |
| `classes/propertyMemberDeclarations/defineProperty.ts` | not supported | expected `:` and a type for the class field, on ` public c; ` |
| `classes/propertyMemberDeclarations/derivedUninitializedPropertyDeclaration.ts` | not supported | `any` is not supported, on ` property: any; // error ` |
| `classes/propertyMemberDeclarations/initializerReferencingConstructorLocals.ts` | not supported | class fields require a type annotation, on ` a = z; // error ` |
| `classes/propertyMemberDeclarations/initializerReferencingConstructorParameters.ts` | not supported | class fields require a type annotation, on ` a = x; // error ` |
| `classes/propertyMemberDeclarations/instanceMemberInitialization.ts` | not supported | construct signatures |
| `classes/propertyMemberDeclarations/instanceMemberWithComputedPropertyName.ts` | not supported | expected class member name, on ` [x] = true; ` |
| `classes/propertyMemberDeclarations/instanceMemberWithComputedPropertyName2.ts` | not supported | expected class member name, on ` [x]: string; ` |
| `classes/propertyMemberDeclarations/memberAccessorDeclarations/accessorsAreNotContextuallyTyped.ts` | not supported | `as` to `C` is not yet supported: class types aren't yet supported as `as` targets, on ` let c: C = null as unknown as (C); ` |
| `classes/propertyMemberDeclarations/memberAccessorDeclarations/accessorWithES5.ts` | not supported | parameter requires a type annotation, on ` set x(v) { ` |
| `classes/propertyMemberDeclarations/memberAccessorDeclarations/accessorWithMismatchedAccessibilityModifiers.ts` | not supported | parameter requires a type annotation, on ` private set x(v) { ` |
| `classes/propertyMemberDeclarations/memberAccessorDeclarations/ambientAccessors.ts` | not supported | expected `;` after expression, on ` declare class C { ` |
| `classes/propertyMemberDeclarations/memberAccessorDeclarations/typeOfThisInAccessor.ts` | not supported | static accessors are not supported, on ` static get y(): number { ` |
| `classes/propertyMemberDeclarations/memberFunctionDeclarations/instanceMemberAssignsToClassPrototype.ts` | not supported | parameter `x` requires a type annotation, on ` C.prototype.bar = (x) => x; // ok ` |
| `classes/propertyMemberDeclarations/memberFunctionDeclarations/memberFunctionOverloadMixingStaticAndInstance.ts` | not supported | expected `:` and return type, on ` foo(); ` |
| `classes/propertyMemberDeclarations/memberFunctionDeclarations/memberFunctionsWithPrivateOverloads.ts` | not supported | expected `:` and return type, on ` private foo(x: number); ` |
| `classes/propertyMemberDeclarations/memberFunctionDeclarations/memberFunctionsWithPublicOverloads.ts` | not supported | expected `:` and return type, on ` public foo(x: number); ` |
| `classes/propertyMemberDeclarations/memberFunctionDeclarations/memberFunctionsWithPublicPrivateOverloads.ts` | not supported | expected `:` and return type, on ` private foo(x: number); ` |
| `classes/propertyMemberDeclarations/memberFunctionDeclarations/typeOfThisInMemberFunctions.ts` | not supported | expected `,` or `>`, on ` class E<T extends Date> { ` |
| `classes/propertyMemberDeclarations/optionalMethod.ts` | checks too little | 0 after the port |
| `classes/propertyMemberDeclarations/optionalProperty.ts` | not supported | expected `:` and a type for the class field, on ` prop?; ` |
| `classes/propertyMemberDeclarations/overrideInterfaceProperty.ts` | not supported | construct signatures |
| `classes/propertyMemberDeclarations/propertyAndAccessorWithSameName.ts` | not supported | parameter requires a type annotation, on ` set x(v) { } // error ` |
| `classes/propertyMemberDeclarations/propertyAndFunctionWithSameName.ts` | not supported | parameter requires a type annotation, on ` x(v): void { } // error ` |
| `classes/propertyMemberDeclarations/propertyNamedConstructor.ts` | not supported | class fields require a type annotation, on ` "constructor" = 3; // Error ` |
| `classes/propertyMemberDeclarations/propertyOverridesAccessors.ts` | not supported | parameter requires a type annotation, on ` set p(value) { this._secret = value } ` |
| `classes/propertyMemberDeclarations/propertyOverridesAccessors2.ts` | not supported | parameter requires a type annotation, on `` set x(value) { console.log(`x was set to ${value}`); } `` |
| `classes/propertyMemberDeclarations/propertyOverridesAccessors3.ts` | not supported | parameter requires a type annotation, on ` set sound(val) { ` |
| `classes/propertyMemberDeclarations/propertyOverridesAccessors4.ts` | not supported | expected `;` after expression, on ` declare class Animal { ` |
| `classes/propertyMemberDeclarations/staticAndNonStaticPropertiesSameName.ts` | checks too little | 0 after the port |
| `classes/propertyMemberDeclarations/staticAutoAccessors.ts` | not supported | expected `:` and a type for the class field, on ` static accessor x: number = 1; ` |
| `classes/propertyMemberDeclarations/staticAutoAccessorsWithDecorators.ts` | not supported | unexpected character `@`, on ` @((t, c) => {}) ` |
| `classes/propertyMemberDeclarations/staticPropertyAndFunctionWithSameName.ts` | checks too little | 0 after the port |
| `classes/propertyMemberDeclarations/staticPropertyNameConflicts.ts` | not supported | expected type, on ` } as const; ` |
| `classes/propertyMemberDeclarations/staticPropertyNameConflictsInAmbientContext.ts` | not supported | declaration files (`.d.ts`) |
| `classes/propertyMemberDeclarations/strictPropertyInitialization.ts` | not supported | unexpected character `#`, on ` #f: number; //Error ` |
| `classes/propertyMemberDeclarations/thisInInstanceMemberInitializer.ts` | not supported | class fields require a type annotation, on ` x = this; ` |
| `classes/propertyMemberDeclarations/thisPropertyOverridesAccessors.ts` | not supported | JavaScript |
| `classes/propertyMemberDeclarations/twoAccessorsWithSameName.ts` | not supported | parameter requires a type annotation, on ` set x(v) {  } ` |
| `classes/propertyMemberDeclarations/twoAccessorsWithSameName2.ts` | not supported | static accessors are not supported, on ` static get x(): number { return 1; } ` |
| `classes/staticIndexSignature/` | not supported | index signatures |
| `constEnums/constEnum1.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum E { ` |
| `constEnums/constEnum2.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum D { ` |
| `constEnums/constEnum3.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum TestType { foo, bar } ` |
| `constEnums/constEnum4.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum A { } ` |
| `constEnums/constEnumNoObjectPrototypePropertyAccess.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum Bebra {} ` |
| `constEnums/constEnumPropertyAccess1.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum G { ` |
| `constEnums/constEnumPropertyAccess2.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum G { ` |
| `constEnums/constEnumPropertyAccess3.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum E { ` |
| `constEnums/importElisionConstEnumMerge1.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` export const enum Enum { ` |
| `controlFlow/assertionTypePredicates1.ts` | not supported | `const` declaration requires an initializer, on ` const assert: (value: unknown) => asserts value = value => {} ` |
| `controlFlow/assertionTypePredicates2.ts` | not supported | JavaScript |
| `controlFlow/controlFlowAliasing2.ts` | not supported | `this` is a reserved keyword and can't be used as a name, on ` function _tcb1(this: { test: Test }): void { ` |
| `controlFlow/controlFlowCommaOperator.ts` | not supported | expected `)`, on ` if (y = "", typeof x === "string") { ` |
| `controlFlow/controlFlowDeleteOperator.ts` | not supported | `delete` |
| `controlFlow/controlFlowElementAccessNoCrash1.ts` | not supported | expected `]` to close array type, on ` commandLineArgs: TestTscCompile["commandLineArgs"]; ` |
| `controlFlow/controlFlowForInStatement.ts` | not supported | `any` is not supported, on ` let obj: any = null as unknown as (any); ` |
| `controlFlow/controlFlowForInStatement2.ts` | not supported | expected `:` after index parameter name, on ` type A = { [keywordA]: number }; ` |
| `controlFlow/controlFlowForOfStatement.ts` | not supported | expected `;` after expression, on ` for (x of obj) { ` |
| `controlFlow/controlFlowGenericTypes.ts` | not supported | expected `,` or `>`, on ` function f1<T extends string \| undefined>(x: T, y: { a: T }, z: [T]): string { ` |
| `controlFlow/controlFlowInstanceOfGuardPrimitives.ts` | not supported | `Date` is not supported, on ` function distinguish(thing: string \| number \| Date): void { ` |
| `controlFlow/controlFlowIterationErrorsAsync.ts` | not supported | expected `;` after expression, on ` async function len(s: string): Promise<number> { ` |
| `controlFlow/controlFlowNullishCoalesce.ts` | the port changes what it checks | `tsc` then reports TS2448 |
| `controlFlow/controlFlowSuperPropertyAccess.ts` | not supported | `protected` is not supported, on ` protected m?(): void; ` |
| `controlFlow/definiteAssignmentAssertions.ts` | not supported | expected `:` and a type for the class field, on ` a!: number; ` |
| `controlFlow/definiteAssignmentAssertionsWithObjectShortHand.ts` | not supported | expected `,` or `}`, on ` const foo = { a! } ` |
| `controlFlow/dependentDestructuredVariablesFromNestedPatterns.ts` | not supported | nested destructuring is not supported, on ` const [[p1, p1Error]] = arg; ` |
| `controlFlow/neverReturningFunctions1.ts` | not supported | construct signatures |
| `controlFlow/switchWithConstrainedTypeVariable.ts` | not supported | expected `,` or `>`, on ` function function1<T extends 'a' \| 'b'>(key: T): void { ` |
| `declarationEmit/anonymousClassAccessorsDeclarationEmit1.ts` | not supported | expected a declaration after `export`, on ` export abstract class Base { ` |
| `declarationEmit/classDoesNotDependOnPrivateMember.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `declarationEmit/declarationEmitWorkWithInlineComments.ts` | checks too little | 0 after the port |
| `declarationEmit/exportDefaultExpressionComments.ts` | not supported | `export default` is not supported, on ` export default null ` |
| `declarationEmit/exportDefaultNamespace.ts` | not supported | `export default` is not supported, on ` export default function someFunc(): string { ` |
| `declarationEmit/leaveOptionalParameterAsWritten.ts` | not supported | expected `;` after expression, on ` declare global { ` |
| `declarationEmit/libReferenceDeclarationEmit.ts` | not supported | empty export specifier list, on ` export {} ` |
| `declarationEmit/libReferenceDeclarationEmitBundle.ts` | not supported | empty export specifier list, on ` export {} ` |
| `declarationEmit/libReferenceNoLib.ts` | not supported | expected a declaration after `export`, on ` export declare interface HTMLElement { field: string; } ` |
| `declarationEmit/libReferenceNoLibBundle.ts` | not supported | expected a declaration after `export`, on ` export declare interface HTMLElement { field: string; } ` |
| `declarationEmit/typeofImportTypeOnlyExport.ts` | not supported | expected `)`, on ` <C>(class_: C) => ` |
| `declarationEmit/typePredicates/declarationEmitIdentifierPredicates01.ts` | not supported | `any` is not supported, on ` export function f(x: any): x is number { ` |
| `declarationEmit/typePredicates/declarationEmitIdentifierPredicatesWithPrivateName01.ts` | not supported | `any` is not supported, on ` export function f(x: any): x is I { ` |
| `declarationEmit/typePredicates/declarationEmitThisPredicates01.ts` | not supported | expected type, on ` m(): this is D { ` |
| `declarationEmit/typePredicates/declarationEmitThisPredicates02.ts` | not supported | expected type, on ` m(): this is Foo { ` |
| `declarationEmit/typePredicates/declarationEmitThisPredicatesWithPrivateName01.ts` | not supported | expected type, on ` m(): this is D { ` |
| `declarationEmit/typePredicates/declarationEmitThisPredicatesWithPrivateName02.ts` | not supported | expected type, on ` m(): this is Foo { ` |
| `declarationEmit/typeReferenceRelatedFiles.ts` | not supported | `node_modules` packages |
| `declarationEmit/typesVersionsDeclarationEmit.ambient.ts` | not supported | `node_modules` packages |
| `declarationEmit/typesVersionsDeclarationEmit.multiFile.ts` | not supported | `node_modules` packages |
| `declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToSelf.ts` | not supported | `node_modules` packages |
| `declarationEmit/typesVersionsDeclarationEmit.multiFileBackReferenceToUnmapped.ts` | not supported | `node_modules` packages |
| `decorators/` | not supported | decorators |
| `directives/ts-expect-error-js.ts` | not supported | JavaScript |
| `directives/ts-expect-error-nocheck-js.ts` | not supported | JavaScript |
| `dynamicImport/` | not supported | dynamic imports |
| `emitter/es2015/asyncGenerators/emitter.asyncGenerators.classMethods.es2015.ts` | not supported | expected `:` and a type for the class field, on ` async * f(): AsyncGenerator<never, void, unknown> { ` |
| `emitter/es2015/asyncGenerators/emitter.asyncGenerators.functionDeclarations.es2015.ts` | not supported | expected `;` after expression, on ` async function * f1(): AsyncGenerator<never, void, unknown> { ` |
| `emitter/es2015/asyncGenerators/emitter.asyncGenerators.functionExpressions.es2015.ts` | not supported | expected `;` after declaration, on ` const f1 = async function * () { ` |
| `emitter/es2015/asyncGenerators/emitter.asyncGenerators.objectLiteralMethods.es2015.ts` | not supported | expected `,` or `}`, on ` async * f() { ` |
| `emitter/es2018/asyncGenerators/emitter.asyncGenerators.classMethods.es2018.ts` | not supported | expected `:` and a type for the class field, on ` async * f(): AsyncGenerator<never, void, unknown> { ` |
| `emitter/es2018/asyncGenerators/emitter.asyncGenerators.functionDeclarations.es2018.ts` | not supported | expected `;` after expression, on ` async function * f1(): AsyncGenerator<never, void, unknown> { ` |
| `emitter/es2018/asyncGenerators/emitter.asyncGenerators.functionExpressions.es2018.ts` | not supported | expected `;` after declaration, on ` const f1 = async function * () { ` |
| `emitter/es2018/asyncGenerators/emitter.asyncGenerators.objectLiteralMethods.es2018.ts` | not supported | expected `,` or `}`, on ` async * f() { ` |
| `emitter/es2019/noCatchBinding/emitter.noCatchBinding.es2019.ts` | checks too little | 0 after the port |
| `emitter/es5/asyncGenerators/emitter.asyncGenerators.classMethods.es5.ts` | not supported | expected `:` and a type for the class field, on ` async * f(): AsyncGenerator<never, void, unknown> { ` |
| `emitter/es5/asyncGenerators/emitter.asyncGenerators.functionDeclarations.es5.ts` | not supported | expected `;` after expression, on ` async function * f1(): AsyncGenerator<never, void, unknown> { ` |
| `emitter/es5/asyncGenerators/emitter.asyncGenerators.functionExpressions.es5.ts` | not supported | expected `;` after declaration, on ` const f1 = async function * () { ` |
| `emitter/es5/asyncGenerators/emitter.asyncGenerators.objectLiteralMethods.es5.ts` | not supported | expected `,` or `}`, on ` async * f() { ` |
| `enums/awaitAndYield.ts` | not supported | expected `;` after expression, on ` async function* test(x: Promise<number>): AsyncGenerator<never, void, unknown> { ` |
| `enums/enumClassification.ts` | not supported | mixed numeric and string enum members are not allowed, on ` D = 10, ` |
| `enums/enumConstantMembers.ts` | not supported | expected `;` after expression, on ` declare enum E4 { ` |
| `enums/enumConstantMemberWithString.ts` | not supported | expected `,` or `}` after enum member, on ` b = "1" + "2", ` |
| `enums/enumConstantMemberWithStringEmitDeclaration.ts` | not supported | expected `,` or `}` after enum member, on ` b = "1" + "2", ` |
| `enums/enumConstantMemberWithTemplateLiterals.ts` | not supported | expected a number or string literal for enum initializer, on `` a = `1` `` |
| `enums/enumConstantMemberWithTemplateLiteralsEmitDeclaration.ts` | not supported | expected a number or string literal for enum initializer, on `` a = `1` `` |
| `enums/enumErrorOnConstantBindingWithInitializer.ts` | not supported | expected a number or string literal for enum initializer, on ` test = value, ` |
| `enums/enumErrors.ts` | not supported | expected a number or string literal for enum initializer, on ` C = new Number(30) ` |
| `enums/enumExportMergingES6.ts` | not supported | expected a number or string literal for enum initializer, on ` CatDog = Cat \| Dog ` |
| `enums/enumMerging.ts` | not supported | expected `;` after expression, on ` namespace M1 { ` |
| `enums/enumMergingErrors.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `enums/enumShadowedInfinityNaN.ts` | not supported | expected a number or string literal for enum initializer, on ` X = Infinity ` |
| `es2017/es2017DateAPIs.ts` | not supported | `Date` is not supported, on ` Date.UTC(2017); ` |
| `es2017/useObjectValuesAndEntries3.ts` | duplicate | of `es2017/useObjectValuesAndEntries2.ts` |
| `es2017/useObjectValuesAndEntries4.ts` | duplicate | of `es2017/useObjectValuesAndEntries2.ts` |
| `es2017/useSharedArrayBuffer2.ts` | duplicate | of `es2017/useSharedArrayBuffer1.ts` |
| `es2017/useSharedArrayBuffer3.ts` | duplicate | of `es2017/useSharedArrayBuffer1.ts` |
| `es2017/useSharedArrayBuffer5.ts` | checks too little | 0 after the port |
| `es2017/useSharedArrayBuffer6.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `es2018/es2018IntlAPIs.ts` | not supported | expected type, on ` const options = { localeMatcher: 'lookup' } as const; ` |
| `es2018/invalidTaggedTemplateEscapeSequences.ts` | not supported | invalid unicode escape: expected hex digits, on `` const x = tag`\u{hello} ${ 100 } \xtraordinary ${ 200 } wonderful ${ 300 } \u... `` |
| `es2019/globalThisAmbientModules.ts` | not supported | expected `;` after expression, on ` declare module "ambientModule" { ` |
| `es2019/globalThisCollision.ts` | not supported | JavaScript |
| `es2019/globalThisGlobalExportAsGlobal.ts` | not supported | expected `;` after expression, on ` declare global { ` |
| `es2019/globalThisPropertyAssignment.ts` | not supported | JavaScript |
| `es2019/globalThisReadonlyProperties.ts` | not supported | `any` is not supported, on ` globalThis.globalThis = 1 as any // should error ` |
| `es2019/globalThisTypeIndexAccess.ts` | not supported | expected `]` to close array type, on ` const w_e: (typeof globalThis)["globalThis"] = null as unknown as ((typeof gl... ` |
| `es2019/globalThisUnknown.ts` | not supported | `let` declaration requires an initializer, on ` let win: Window & typeof globalThis = null as unknown as (Window & typeof glo... ` |
| `es2019/globalThisUnknownNoImplicitAny.ts` | not supported | `let` declaration requires an initializer, on ` let win: Window & typeof globalThis = null as unknown as (Window & typeof glo... ` |
| `es2019/globalThisVarDeclaration.ts` | not supported | JavaScript |
| `es2019/importMeta/importMeta.ts` | not supported | expected `)`, on ` (async () => { ` |
| `es2019/importMeta/importMetaNarrowing.ts` | not supported | expected `;` after expression, on ` declare global { interface ImportMeta {foo?: () => void} }; ` |
| `es2020/bigintMissingES2019.ts` | not supported | expected `,` or `>`, on ` function test<A, B extends A>(): void { } ` |
| `es2020/bigintMissingES2020.ts` | not supported | expected `,` or `>`, on ` function test<A, B extends A>(): void { } ` |
| `es2020/bigintMissingESNext.ts` | not supported | expected `,` or `>`, on ` function test<A, B extends A>(): void { } ` |
| `es2020/intlNumberFormatES2020.ts` | not supported | unknown type `Intl.NumberFormatPartTypes`, on ` const types: Intl.NumberFormatPartTypes[] = [ 'compact', 'unit', 'unknown' ]; ` |
| `es2020/modules/exportAsNamespace_exportAssignment.ts` | not supported | expected a declaration after `export`, on ` export = {} ` |
| `es2020/modules/exportAsNamespace_missingEmitHelpers.ts` | not supported | empty export specifier list, on ` export {} ` |
| `es2020/modules/exportAsNamespace_nonExistent.ts` | not supported | expected a declaration after `export`, on ` export * as ns from './nonexistent'; // Error ` |
| `es2020/modules/exportAsNamespace1.ts` | not supported | expected a declaration after `export`, on ` export * as ns from './0'; ` |
| `es2020/modules/exportAsNamespace2.ts` | not supported | expected a declaration after `export`, on ` export * as ns from './0'; ` |
| `es2020/modules/exportAsNamespace4.ts` | not supported | expected a declaration after `export`, on ` export * as default from './0'; ` |
| `es2020/modules/exportAsNamespace5.ts` | not supported | declaration files (`.d.ts`) |
| `es2021/es2021LocalesObjectArgument.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `es2021/intlDateTimeFormatRangeES2021.ts` | not supported | `Date` is not supported, on ` new Intl.DateTimeFormat().formatRange(new Date(0), new Date()); ` |
| `es2021/logicalAssignment/logicalAssignment1.ts` | not supported | expected expression, on ` a &&= "foo" ` |
| `es2021/logicalAssignment/logicalAssignment10.ts` | not supported | expected expression, on ` return ++count; ` |
| `es2021/logicalAssignment/logicalAssignment3.ts` | not supported | expected expression, on ` (a.baz) &&= result.baz; ` |
| `es2021/logicalAssignment/logicalAssignment4.ts` | not supported | expected expression, on ` (results \|\|= []).push(100); ` |
| `es2021/logicalAssignment/logicalAssignment6.ts` | not supported | expected expression, on ` (results \|\|= (results1 \|\|= [])).push(100); ` |
| `es2021/logicalAssignment/logicalAssignment7.ts` | not supported | expected expression, on ` (results \|\|= results1 \|\|= []).push(100); ` |
| `es2021/logicalAssignment/logicalAssignment8.ts` | not supported | expected expression, on ` (results \|\|= bar?.value ?? []).push(100); ` |
| `es2021/logicalAssignment/logicalAssignment9.ts` | not supported | expected expression, on ` x.a ??= true; ` |
| `es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_exportEmpty.ts` | not supported | expected local name after `as`, on ` export { empty as "" }; ` |
| `es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_importEmpty.ts` | not supported | expected import specifier name, on ` "missing" as x, ` |
| `es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts` | not supported | expected local name after `as`, on ` export { someValue as "<X>" }; ` |
| `es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_syntax.ts` | not supported | expected local name after `as`, on ` export { foo as "valid 1" }; ` |
| `es2022/es2022IntlAPIs.ts` | not supported | expected type, on ` const timezoneNames = ['short', 'long', 'shortOffset', 'longOffset', 'shortGe... ` |
| `es2022/es2022LocalesObjectArgument.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `es2023/intlNumberFormatES2023.ts` | the port changes what it checks | `tsc` then reports TS2339, TS2769, TS2551, TS7006 |
| `es2023/intlNumberFormatES5UseGrouping.ts` | the port changes what it checks | `tsc` then reports TS2769 |
| `es2024/resizableArrayBuffer.ts` | the port changes what it checks | `tsc` then reports TS2554, TS2550 |
| `es2024/sharedMemory.ts` | not supported | expected `;` after declaration, on ` const main = async () => { ` |
| `es2024/transferableArrayBuffer.ts` | the port changes what it checks | `tsc` then reports TS2550 |
| `es2025/float16Array.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `es2025/intlDurationFormat.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `es2025/regExpEscape.ts` | the port changes what it checks | `tsc` then reports TS2339 |
| `es2025/syncIteratorHelpers.ts` | the port changes what it checks | `tsc` then reports TS2339, TS7006 |
| `es5/es5DateAPIs.ts` | not supported | `Date` is not supported, on ` Date.UTC(2017); // should error ` |
| `es6/arrowFunction/emitArrowFunctionAsIs.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionAsIsES6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionsAsIs.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionsAsIsES6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionThisCapturingES6.ts` | duplicate | of `es6/arrowFunction/emitArrowFunctionThisCapturing.ts` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments01_ES6.ts` | duplicate | of `es6/arrowFunction/emitArrowFunctionWhenUsingArguments01.ts` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments02_ES6.ts` | duplicate | of `es6/arrowFunction/emitArrowFunctionWhenUsingArguments02.ts` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments03_ES6.ts` | not supported | `let` declaration requires an initializer, on ` let arguments; ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments03.ts` | not supported | `let` declaration requires an initializer, on ` let arguments; ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments04_ES6.ts` | not supported | `let` declaration requires an initializer, on ` let arguments; ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments04.ts` | not supported | `let` declaration requires an initializer, on ` let arguments; ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments05_ES6.ts` | not supported | parameter requires a type annotation, on ` function f(arguments): void { ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments05.ts` | not supported | parameter requires a type annotation, on ` function f(arguments): void { ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments06_ES6.ts` | not supported | parameter requires a type annotation, on ` function f(arguments): void { ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments06.ts` | not supported | parameter requires a type annotation, on ` function f(arguments): void { ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments07_ES6.ts` | not supported | parameter requires a type annotation, on ` function f(arguments): void { ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments07.ts` | not supported | parameter requires a type annotation, on ` function f(arguments): void { ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments08_ES6.ts` | not supported | parameter requires a type annotation, on ` function f(arguments): void { ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments08.ts` | not supported | parameter requires a type annotation, on ` function f(arguments): void { ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments09_ES6.ts` | not supported | parameter requires a type annotation, on ` function f(_arguments): void { ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments09.ts` | not supported | parameter requires a type annotation, on ` function f(_arguments): void { ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments10_ES6.ts` | duplicate | of `es6/arrowFunction/emitArrowFunctionWhenUsingArguments10.ts` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments11_ES6.ts` | not supported | parameter requires a type annotation, on ` function f(arguments): void { ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments11.ts` | not supported | parameter requires a type annotation, on ` function f(arguments): void { ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments12_ES6.ts` | not supported | parameter requires a type annotation, on ` f(arguments): void { ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments12.ts` | not supported | parameter requires a type annotation, on ` f(arguments): void { ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments13_ES6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments13.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments15_ES6.ts` | duplicate | of `es6/arrowFunction/emitArrowFunctionWhenUsingArguments15.ts` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments16_ES6.ts` | duplicate | of `es6/arrowFunction/emitArrowFunctionWhenUsingArguments16.ts` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments17_ES6.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments17.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments18_ES6.ts` | not supported | unknown type `IArguments`, on ` function f(): (() => IArguments) \| undefined { ` |
| `es6/arrowFunction/emitArrowFunctionWhenUsingArguments18.ts` | not supported | unknown type `IArguments`, on ` function f(): (() => IArguments) \| undefined { ` |
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
| `es6/classDeclaration/emitClassDeclarationWithConstructorInES6.ts` | not supported | `any` is not supported, on ` foo(a: any); ` |
| `es6/classDeclaration/emitClassDeclarationWithExtensionAndTypeArgumentInES6.ts` | not supported | `any` is not supported, on ` constructor(a: any) ` |
| `es6/classDeclaration/emitClassDeclarationWithGetterSetterInES6.ts` | not supported | static accessors are not supported, on ` static get name2(): string { ` |
| `es6/classDeclaration/emitClassDeclarationWithLiteralPropertyNameInES6.ts` | not supported | class fields require a type annotation, on ` "hello" = 10; ` |
| `es6/classDeclaration/emitClassDeclarationWithMethodInES6.ts` | not supported | expected class member name, on ` ["computedName1"](): void { } ` |
| `es6/classDeclaration/emitClassDeclarationWithPropertyAccessInHeritageClause1.ts` | not supported | expected `)` to close a parenthesized type, on ` class C extends (foo()).B {} ` |
| `es6/classDeclaration/emitClassDeclarationWithTypeArgumentAndOverloadInES6.ts` | not supported | `any` is not supported, on ` constructor(a: any) ` |
| `es6/classDeclaration/exportDefaultClassWithStaticPropertyAssignmentsInES6.ts` | not supported | `export default` is not supported, on ` export default class { ` |
| `es6/classDeclaration/parseClassDeclarationInStrictModeByDefaultInES6.ts` | not supported | `any` is not supported, on ` public foo(arguments: any): void { } ` |
| `es6/classDeclaration/superCallBeforeThisAccessing1.ts` | not supported | `any` is not supported, on ` let Factory: any = null as unknown as (any); ` |
| `es6/classDeclaration/superCallBeforeThisAccessing2.ts` | not supported | parameter requires a type annotation, on ` constructor(c) { } ` |
| `es6/classDeclaration/superCallBeforeThisAccessing3.ts` | not supported | parameter requires a type annotation, on ` constructor(c) { } ` |
| `es6/classDeclaration/superCallBeforeThisAccessing4.ts` | not supported | expected `:` and a type for the class field, on ` private _t; ` |
| `es6/classDeclaration/superCallBeforeThisAccessing5.ts` | not supported | expected `:` and a type for the class field, on ` private _t; ` |
| `es6/classDeclaration/superCallBeforeThisAccessing6.ts` | not supported | parameter requires a type annotation, on ` constructor(c) { } ` |
| `es6/classDeclaration/superCallBeforeThisAccessing7.ts` | not supported | parameter requires a type annotation, on ` constructor(c) { } ` |
| `es6/classDeclaration/superCallBeforeThisAccessing8.ts` | not supported | parameter requires a type annotation, on ` constructor(c) { } ` |
| `es6/classExpressions/classExpressionES61.ts` | not supported | expected expression, on ` let v = class C {}; ` |
| `es6/classExpressions/classExpressionES62.ts` | not supported | expected expression, on ` let v = class C extends D {}; ` |
| `es6/classExpressions/classExpressionES63.ts` | not supported | expected expression, on ` let C = class extends class extends class { a: number = 1 } { b: number = 2 }... ` |
| `es6/classExpressions/typeArgumentInferenceWithClassExpression1.ts` | not supported | expected expression, on ` function foo<T>(x = class { static prop: T }): T { ` |
| `es6/classExpressions/typeArgumentInferenceWithClassExpression2.ts` | not supported | expected expression, on ` function foo<T>(x = class { prop: T }): T { ` |
| `es6/classExpressions/typeArgumentInferenceWithClassExpression3.ts` | not supported | expected expression, on ` function foo<T>(x = class { prop: T }): T { ` |
| `es6/computedProperties/` | not supported | computed property names |
| `es6/decorators/` | not supported | decorators |
| `es6/defaultParameters/emitDefaultParametersFunction.ts` | not supported | rest parameter requires a type annotation, on ` function baz(x: string, y: number = 5, ...rest): void { } ` |
| `es6/defaultParameters/emitDefaultParametersFunctionES6.ts` | not supported | rest parameter requires a type annotation, on ` function baz(x: string, y: number = 5, ...rest): void { } ` |
| `es6/defaultParameters/emitDefaultParametersFunctionExpression.ts` | not supported | parameter `rest` requires a type annotation, on ` let y = (function (num: number = 10, boo: boolean = false, ...rest) { })() ` |
| `es6/defaultParameters/emitDefaultParametersFunctionExpressionES6.ts` | not supported | parameter `rest` requires a type annotation, on ` let y = (function (num: number = 10, boo: boolean = false, ...rest) { })() ` |
| `es6/defaultParameters/emitDefaultParametersFunctionProperty.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/defaultParameters/emitDefaultParametersFunctionPropertyES6.ts` | the port changes what it checks | `tsc` then reports TS7019 |
| `es6/defaultParameters/emitDefaultParametersMethod.ts` | not supported | rest parameter requires a type annotation, on ` public foo1(x: string, t: boolean = false, ...rest): void { } ` |
| `es6/defaultParameters/emitDefaultParametersMethodES6.ts` | not supported | rest parameter requires a type annotation, on ` public foo1(x: string, t: boolean = false, ...rest): void { } ` |
| `es6/destructuring/arrayAssignmentPatternWithAny.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/declarationInAmbientContext.ts` | not supported | `let` declaration requires an initializer, on ` let [a, b];  // Error, destructuring declaration not allowed in ambient context ` |
| `es6/destructuring/declarationsAndAssignments.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `es6/destructuring/declarationWithNoInitializer.ts` | not supported | `let` declaration requires an initializer, on ` let [a, b];          // Error, no initializer ` |
| `es6/destructuring/destructuringArrayBindingPatternAndAssignment1ES5iterable.ts` | duplicate | of `es6/destructuring/destructuringArrayBindingPatternAndAssignment1ES5.ts` |
| `es6/destructuring/destructuringArrayBindingPatternAndAssignment1ES6.ts` | duplicate | of `es6/destructuring/destructuringArrayBindingPatternAndAssignment1ES5.ts` |
| `es6/destructuring/destructuringArrayBindingPatternAndAssignment2.ts` | not supported | nested destructuring is not supported, on ` let [[a0], [[a1]]] = []         // Error ` |
| `es6/destructuring/destructuringArrayBindingPatternAndAssignment3.ts` | not supported | parameter `#pattern_p_0` requires a type annotation, on ` (function ([a, b = a]) { // ok ` |
| `es6/destructuring/destructuringAssignabilityCheck.ts` | not supported | empty array destructuring pattern, on ` const [] = {}; // should be error ` |
| `es6/destructuring/destructuringCatch.ts` | not supported | expected catch binding name, on ` catch ([a, b]) { ` |
| `es6/destructuring/destructuringEvaluationOrder.ts` | not supported | `any` is not supported, on ` let trace: any[] = []; ` |
| `es6/destructuring/destructuringInFunctionType.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` interface a { a } ` |
| `es6/destructuring/destructuringObjectAssignmentPatternWithNestedSpread.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any), b: any = null as unknown as (any), c: ... ` |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment1ES5.ts` | not supported | `any` is not supported, on ` let { a1 }: any = undefined; ` |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment1ES6.ts` | not supported | `any` is not supported, on ` let { a1 }: any = undefined; ` |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment3.ts` | not supported | expected `,` or `}` in object pattern, on ` let {h?} = { h?: 1 }; ` |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment4.ts` | not supported | `any` is not supported, on ` } = { } as any; ` |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment5.ts` | not supported | `any` is not supported, on ` let y: any = null as unknown as (any); ` |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment6.ts` | not supported | expected field name in object pattern, on ` const { [a]: aVal, [b]: bVal } = (() => { ` |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment7.ts` | not supported | expected field name in object pattern, on ` const { [K.a]: aVal, [K.b]: bVal } = (() => { ` |
| `es6/destructuring/destructuringObjectBindingPatternAndAssignment8.ts` | not supported | expected field name in object pattern, on ` const { [K.a]: aVal, [K.b]: bVal } = (() => { ` |
| `es6/destructuring/destructuringParameterDeclaration10.ts` | not supported | nested destructuring is not supported, on ` additionalFiles: { ` |
| `es6/destructuring/destructuringParameterDeclaration1ES5iterable.ts` | duplicate | of `es6/destructuring/destructuringParameterDeclaration1ES5.ts` |
| `es6/destructuring/destructuringParameterDeclaration2.ts` | not supported | nested destructuring is not supported, on ` function a0([a, b, [[c]]]: [number, number, string[][]]): void { } ` |
| `es6/destructuring/destructuringParameterDeclaration3ES6.ts` | duplicate | of `es6/destructuring/destructuringParameterDeclaration3ES5iterable.ts` |
| `es6/destructuring/destructuringParameterDeclaration5.ts` | not supported | expected `,` or `>`, on ` function d0<T extends Class>({x} = { x: new Class() }): void { } ` |
| `es6/destructuring/destructuringParameterDeclaration6.ts` | not supported | expected `:` after keyword field name in object pattern, on ` function a({while}): void { } ` |
| `es6/destructuring/destructuringParameterDeclaration7ES5.ts` | not supported | empty object destructuring pattern, on ` function foo({}, {foo, bar}: ISomething): void {} ` |
| `es6/destructuring/destructuringParameterDeclaration7ES5iterable.ts` | not supported | empty object destructuring pattern, on ` function foo({}, {foo, bar}: ISomething): void {} ` |
| `es6/destructuring/destructuringParameterDeclaration8.ts` | not supported | nested destructuring is not supported, on ` nested: { p = 'c' } ` |
| `es6/destructuring/destructuringParameterDeclaration9.ts` | not supported | JavaScript |
| `es6/destructuring/destructuringParameterProperties1.ts` | not supported | `public` is a reserved word in strict mode and can't be used as a name, on ` constructor(public [x, y, z]: string[]) { ` |
| `es6/destructuring/destructuringParameterProperties2.ts` | not supported | `private` is a reserved word in strict mode and can't be used as a name, on ` constructor(private k: number, private [a, b, c]: [number, string, boolean]) { ` |
| `es6/destructuring/destructuringParameterProperties3.ts` | not supported | `private` is a reserved word in strict mode and can't be used as a name, on ` constructor(private k: T, private [a, b, c]: [T,U,V]) { ` |
| `es6/destructuring/destructuringParameterProperties4.ts` | not supported | `protected` is a reserved word in strict mode and can't be used as a name, on ` constructor(private k: T, protected [a, b, c]: [T,U,V]) { ` |
| `es6/destructuring/destructuringParameterProperties5.ts` | not supported | `public` is a reserved word in strict mode and can't be used as a name, on ` constructor(public [{ x1, x2, x3 }, y, z]: TupleType1) { ` |
| `es6/destructuring/destructuringReassignsRightHandSide.ts` | not supported | `any` is not supported, on ` let foo: any = { foo: 1, bar: 2 }; ` |
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
| `es6/destructuring/emptyArrayBindingPatternParameter01.ts` | not supported | empty array destructuring pattern, on ` function f([]): void { ` |
| `es6/destructuring/emptyArrayBindingPatternParameter02.ts` | not supported | parameter requires a type annotation, on ` function f(a, []): void { ` |
| `es6/destructuring/emptyArrayBindingPatternParameter03.ts` | not supported | parameter requires a type annotation, on ` function f(a, []): void { ` |
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
| `es6/destructuring/emptyObjectBindingPatternParameter02.ts` | not supported | parameter requires a type annotation, on ` function f(a, {}): void { ` |
| `es6/destructuring/emptyObjectBindingPatternParameter03.ts` | not supported | empty object destructuring pattern, on ` function f({}, a): void { ` |
| `es6/destructuring/emptyObjectBindingPatternParameter04.ts` | not supported | empty object destructuring pattern, on ` function f({} = {a: 1, b: "2", c: true}): void { ` |
| `es6/destructuring/emptyVariableDeclarationBindingPatterns01_ES5.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyVariableDeclarationBindingPatterns01_ES5iterable.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyVariableDeclarationBindingPatterns01_ES6.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es6/destructuring/emptyVariableDeclarationBindingPatterns02_ES5.ts` | not supported | empty object destructuring pattern, on ` let {}; ` |
| `es6/destructuring/emptyVariableDeclarationBindingPatterns02_ES5iterable.ts` | not supported | empty object destructuring pattern, on ` let {}; ` |
| `es6/destructuring/emptyVariableDeclarationBindingPatterns02_ES6.ts` | not supported | empty object destructuring pattern, on ` let {}; ` |
| `es6/destructuring/iterableArrayPattern1.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/destructuring/iterableArrayPattern10.ts` | not supported | expected `:` and a type for the class field, on ` class Bar { x } ` |
| `es6/destructuring/iterableArrayPattern11.ts` | not supported | expected `:` and a type for the class field, on ` class Bar { x } ` |
| `es6/destructuring/iterableArrayPattern12.ts` | not supported | expected `:` and a type for the class field, on ` class Bar { x } ` |
| `es6/destructuring/iterableArrayPattern13.ts` | not supported | expected `:` and a type for the class field, on ` class Bar { x } ` |
| `es6/destructuring/iterableArrayPattern14.ts` | not supported | expected `:` and a type for the class field, on ` class Bar { x } ` |
| `es6/destructuring/iterableArrayPattern15.ts` | not supported | expected `:` and a type for the class field, on ` class Bar { x } ` |
| `es6/destructuring/iterableArrayPattern16.ts` | not supported | rest parameter cannot be destructured, on ` function fun(...[a, b]: [Bar, Bar][]): void { } ` |
| `es6/destructuring/iterableArrayPattern17.ts` | not supported | expected `:` and a type for the class field, on ` class Bar { x } ` |
| `es6/destructuring/iterableArrayPattern18.ts` | not supported | expected `:` and a type for the class field, on ` class Bar { x } ` |
| `es6/destructuring/iterableArrayPattern19.ts` | not supported | expected `:` and a type for the class field, on ` class Bar { x } ` |
| `es6/destructuring/iterableArrayPattern2.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `es6/destructuring/iterableArrayPattern20.ts` | not supported | expected `:` and a type for the class field, on ` class Bar { x } ` |
| `es6/destructuring/iterableArrayPattern21.ts` | not supported | expected field name, on ` let [a, b] = { 0: "", 1: true }; ` |
| `es6/destructuring/iterableArrayPattern22.ts` | not supported | expected field name, on ` let [...a] = { 0: "", 1: true }; ` |
| `es6/destructuring/iterableArrayPattern23.ts` | not supported | expected `;` after declaration, on ` let a: string = null as unknown as (string), b: boolean = null as unknown as ... ` |
| `es6/destructuring/iterableArrayPattern24.ts` | not supported | expected `;` after declaration, on ` let a: string = null as unknown as (string), b: boolean[] = null as unknown a... ` |
| `es6/destructuring/iterableArrayPattern25.ts` | not supported | rest parameter cannot be destructured, on ` function takeFirstTwoEntries(...[[k1, v1], [k2, v2]]): void { } ` |
| `es6/destructuring/iterableArrayPattern26.ts` | not supported | rest parameter cannot be destructured, on ` function takeFirstTwoEntries(...[[k1, v1], [k2, v2]]: [string, number][]): vo... ` |
| `es6/destructuring/iterableArrayPattern27.ts` | not supported | rest parameter cannot be destructured, on ` function takeFirstTwoEntries(...[[k1, v1], [k2, v2]]: [string, number][]): vo... ` |
| `es6/destructuring/iterableArrayPattern28.ts` | not supported | rest parameter cannot be destructured, on ` function takeFirstTwoEntries(...[[k1, v1], [k2, v2]]: [string, number][]): vo... ` |
| `es6/destructuring/iterableArrayPattern29.ts` | not supported | rest parameter cannot be destructured, on ` function takeFirstTwoEntries(...[[k1, v1], [k2, v2]]: [string, number][]): vo... ` |
| `es6/destructuring/iterableArrayPattern3.ts` | not supported | expected `:` and a type for the class field, on ` class Bar { x } ` |
| `es6/destructuring/iterableArrayPattern30.ts` | not supported | nested destructuring is not supported, on ` const [[k1, v1], [k2, v2]] = new Map([["", true], ["hello", true]]) ` |
| `es6/destructuring/iterableArrayPattern4.ts` | not supported | expected `:` and a type for the class field, on ` class Bar { x } ` |
| `es6/destructuring/iterableArrayPattern5.ts` | not supported | expected `:` and a type for the class field, on ` class Bar { x } ` |
| `es6/destructuring/iterableArrayPattern6.ts` | not supported | expected `:` and a type for the class field, on ` class Bar { x } ` |
| `es6/destructuring/iterableArrayPattern7.ts` | not supported | expected `:` and a type for the class field, on ` class Bar { x } ` |
| `es6/destructuring/iterableArrayPattern8.ts` | not supported | expected `:` and a type for the class field, on ` class Bar { x } ` |
| `es6/destructuring/iterableArrayPattern9.ts` | not supported | expected `(` after constructor name in `new` expression, on ` function fun([a, b] = new FooIterator): void { } ` |
| `es6/destructuring/missingAndExcessProperties.ts` | not supported | expected `;` after declaration, on ` let x: number = null as unknown as (number), y: number = null as unknown as (... ` |
| `es6/destructuring/nonIterableRestElement1.ts` | not supported | invalid assignment target, on ` [...c] = ["", 0]; ` |
| `es6/destructuring/nonIterableRestElement2.ts` | not supported | invalid assignment target, on ` [...c] = ["", 0]; ` |
| `es6/destructuring/nonIterableRestElement3.ts` | not supported | invalid assignment target, on ` [...c] = ["", 0]; ` |
| `es6/destructuring/objectBindingPatternKeywordIdentifiers01.ts` | not supported | expected `:` after keyword field name in object pattern, on ` let { while } = { while: 1 } ` |
| `es6/destructuring/objectBindingPatternKeywordIdentifiers02.ts` | not supported | `while` is a reserved keyword and can't be used as a name, on ` let { while: while } = { while: 1 } ` |
| `es6/destructuring/objectBindingPatternKeywordIdentifiers03.ts` | not supported | expected field name in object pattern, on ` let { "while" } = { while: 1 } ` |
| `es6/destructuring/objectBindingPatternKeywordIdentifiers04.ts` | not supported | expected field name in object pattern, on ` let { "while": while } = { while: 1 } ` |
| `es6/destructuring/optionalBindingParameters1.ts` | not supported | a destructured parameter cannot be optional; use a default value, on ` function foo([x,y,z]?: [string, number, boolean]): void { ` |
| `es6/destructuring/optionalBindingParameters2.ts` | not supported | a destructured parameter cannot be optional; use a default value, on ` function foo({ x, y, z }?: { x: string; y: number; z: boolean }): void { ` |
| `es6/destructuring/optionalBindingParameters3.ts` | not supported | JavaScript |
| `es6/destructuring/optionalBindingParameters4.ts` | not supported | JavaScript |
| `es6/destructuring/optionalBindingParametersInOverloads1.ts` | not supported | a destructured parameter cannot be optional; use a default value, on ` function foo([x, y, z] ?: [string, number, boolean]); ` |
| `es6/destructuring/optionalBindingParametersInOverloads2.ts` | not supported | a destructured parameter cannot be optional; use a default value, on ` function foo({ x, y, z }?: { x: string; y: number; z: boolean }); ` |
| `es6/destructuring/restElementWithAssignmentPattern1.ts` | not supported | expected `;` after declaration, on ` let a: string = null as unknown as (string), b: number = null as unknown as (... ` |
| `es6/destructuring/restElementWithAssignmentPattern2.ts` | not supported | expected `;` after declaration, on ` let a: string = null as unknown as (string), b: number = null as unknown as (... ` |
| `es6/destructuring/restElementWithAssignmentPattern3.ts` | not supported | expected `;` after declaration, on ` let a: string = null as unknown as (string), b: number = null as unknown as (... ` |
| `es6/destructuring/restElementWithAssignmentPattern4.ts` | not supported | expected `;` after declaration, on ` let a: string = null as unknown as (string), b: number = null as unknown as (... ` |
| `es6/destructuring/restElementWithAssignmentPattern5.ts` | not supported | expected `;` after declaration, on ` let s: string = null as unknown as (string), s2: string = null as unknown as ... ` |
| `es6/destructuring/restElementWithBindingPattern.ts` | not supported | expected identifier after `...`, on ` let [...[a, b]] = [0, 1]; ` |
| `es6/destructuring/restElementWithBindingPattern2.ts` | not supported | expected identifier after `...`, on ` let [...{0: a, b }] = [0, 1]; ` |
| `es6/destructuring/restElementWithInitializer2.ts` | not supported | invalid assignment target, on ` [...x = a] = a;  // Error, rest element cannot have initializer ` |
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
| `es6/for-ofStatements/for-of22.ts` | not supported | expected `(` after constructor name in `new` expression, on ` value: new Foo, ` |
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
| `es6/for-ofStatements/for-of33.ts` | not supported | expected class member name, on ` [Symbol.iterator](): any { ` |
| `es6/for-ofStatements/for-of34.ts` | not supported | `any` is not supported, on ` next(): any { ` |
| `es6/for-ofStatements/for-of35.ts` | not supported | `any` is not supported, on ` next(): { done: boolean; value: any; } { ` |
| `es6/for-ofStatements/for-of41.ts` | not supported | nested destructuring is not supported, on ` for (let {x: [a], y: {p}} of array) { ` |
| `es6/for-ofStatements/for-of42.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (let {x: a, y: b} of array) { ` |
| `es6/for-ofStatements/for-of43.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (let {x: a = "", y: b = true} of array) { ` |
| `es6/for-ofStatements/for-of44.ts` | not supported | unknown type `symbol`, on ` let array: [number, string \| boolean \| symbol][] = [[0, ""], [0, true], [1, S... ` |
| `es6/for-ofStatements/for-of45.ts` | not supported | expected `;` after declaration, on ` let k: string = null as unknown as (string), v: boolean = null as unknown as ... ` |
| `es6/for-ofStatements/for-of46.ts` | not supported | expected `;` after declaration, on ` let k: string = null as unknown as (string), v: boolean = null as unknown as ... ` |
| `es6/for-ofStatements/for-of47.ts` | not supported | expected `;` after declaration, on ` let x: string = null as unknown as (string), y: number = null as unknown as (... ` |
| `es6/for-ofStatements/for-of48.ts` | not supported | expected `;` after declaration, on ` let x: string = null as unknown as (string), y: number = null as unknown as (... ` |
| `es6/for-ofStatements/for-of49.ts` | not supported | expected `;` after declaration, on ` let k: string = null as unknown as (string), v: boolean = null as unknown as ... ` |
| `es6/for-ofStatements/for-of5.ts` | duplicate | of `es6/for-ofStatements/for-of4.ts` |
| `es6/for-ofStatements/for-of51.ts` | not supported | `let` is a reserved keyword and can't be used as a name, on ` for (let let of []) {} ` |
| `es6/for-ofStatements/for-of53.ts` | not supported | `let` declaration requires an initializer, on ` let v; ` |
| `es6/for-ofStatements/for-of56.ts` | not supported | `let` is a reserved keyword and can't be used as a name, on ` for (let let of []) {} ` |
| `es6/for-ofStatements/for-of57.ts` | not supported | `as` to `Iterable<number>` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let iter: Iterable<number> = null as unknown as (Iterable<number>); ` |
| `es6/for-ofStatements/for-of58.ts` | not supported | `const` declaration requires an initializer, on ` const arr: X[] & Y[] = null as unknown as (X[] & Y[]); ` |
| `es6/for-ofStatements/for-of6.ts` | not supported | expected `;` after expression, on ` for (v of [0]) { ` |
| `es6/for-ofStatements/for-of8.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `es6/for-ofStatements/for-of9.ts` | not supported | expected `;` after expression, on ` for (v of ["hello"]) { } ` |
| `es6/functionDeclarations/FunctionDeclaration1_es6.ts` | not supported | expected function name, on ` function * foo(): Generator<never, void, unknown> { ` |
| `es6/functionDeclarations/FunctionDeclaration10_es6.ts` | not supported | expected function name, on ` function * foo(a = yield => yield): Generator<never, void, unknown> { ` |
| `es6/functionDeclarations/FunctionDeclaration11_es6.ts` | not supported | expected function name, on ` function * yield(): Generator<never, void, unknown> { ` |
| `es6/functionDeclarations/FunctionDeclaration12_es6.ts` | not supported | expected `(` after `function`, on ` let v = function * yield() { } ` |
| `es6/functionDeclarations/FunctionDeclaration13_es6.ts` | not supported | expected function name, on ` function * foo(): Generator<never, void, unknown> { ` |
| `es6/functionDeclarations/FunctionDeclaration2_es6.ts` | not supported | `yield` is a reserved word in strict mode and can't be used as a name, on ` function f(yield): void { ` |
| `es6/functionDeclarations/FunctionDeclaration3_es6.ts` | not supported | `yield` is a reserved word in strict mode and can't be used as a name, on ` function f(yield = yield): void { ` |
| `es6/functionDeclarations/FunctionDeclaration4_es6.ts` | not supported | `yield` is a reserved word in strict mode and can't be used as a name, on ` function yield(): void { ` |
| `es6/functionDeclarations/FunctionDeclaration5_es6.ts` | not supported | expected function name, on ` function*foo(yield): Generator<never, void, unknown> { ` |
| `es6/functionDeclarations/FunctionDeclaration6_es6.ts` | not supported | expected function name, on ` function*foo(a = yield): Generator<never, void, unknown> { ` |
| `es6/functionDeclarations/FunctionDeclaration7_es6.ts` | not supported | expected function name, on ` function*bar(): Generator<never, void, unknown> { ` |
| `es6/functionDeclarations/FunctionDeclaration9_es6.ts` | not supported | expected function name, on ` function * foo(): Generator<undefined, void, unknown> { ` |
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
| `es6/moduleExportsAmd/anonymousDefaultExportsAmd.ts` | not supported | `export default` is not supported, on ` export default class {} ` |
| `es6/moduleExportsAmd/decoratedDefaultExportsGetExportedAmd.ts` | not supported | unexpected character `@`, on ` @decorator ` |
| `es6/moduleExportsAmd/defaultExportsGetExportedAmd.ts` | not supported | `export default` is not supported, on ` export default class Foo {} ` |
| `es6/moduleExportsAmd/outFilerootDirModuleNamesAmd.ts` | not supported | `export default` is not supported, on ` export default class Foo {} ` |
| `es6/moduleExportsCommonjs/anonymousDefaultExportsCommonjs.ts` | not supported | `export default` is not supported, on ` export default class {} ` |
| `es6/moduleExportsCommonjs/decoratedDefaultExportsGetExportedCommonjs.ts` | not supported | unexpected character `@`, on ` @decorator ` |
| `es6/moduleExportsCommonjs/defaultExportsGetExportedCommonjs.ts` | not supported | `export default` is not supported, on ` export default class Foo {} ` |
| `es6/moduleExportsSystem/anonymousDefaultExportsSystem.ts` | not supported | `export default` is not supported, on ` export default class {} ` |
| `es6/moduleExportsSystem/decoratedDefaultExportsGetExportedSystem.ts` | not supported | unexpected character `@`, on ` @decorator ` |
| `es6/moduleExportsSystem/defaultExportsGetExportedSystem.ts` | not supported | `export default` is not supported, on ` export default class Foo {} ` |
| `es6/moduleExportsSystem/outFilerootDirModuleNamesSystem.ts` | not supported | `export default` is not supported, on ` export default class Foo {} ` |
| `es6/moduleExportsSystem/topLevelVarHoistingCommonJS.ts` | not supported | `any` is not supported, on ` let _: any = null as unknown as (any); ` |
| `es6/moduleExportsSystem/topLevelVarHoistingSystem.ts` | the port changes what it checks | `tsc` then reports TS2304 |
| `es6/moduleExportsUmd/anonymousDefaultExportsUmd.ts` | not supported | `export default` is not supported, on ` export default class {} ` |
| `es6/moduleExportsUmd/decoratedDefaultExportsGetExportedUmd.ts` | not supported | unexpected character `@`, on ` @decorator ` |
| `es6/moduleExportsUmd/defaultExportsGetExportedUmd.ts` | not supported | `export default` is not supported, on ` export default class Foo {} ` |
| `es6/modules/defaultExportInAwaitExpression01.ts` | not supported | `export default` is not supported, on ` export default x; ` |
| `es6/modules/defaultExportInAwaitExpression02.ts` | not supported | `export default` is not supported, on ` export default x; ` |
| `es6/modules/defaultExportsCannotMerge01.ts` | not supported | `export default` is not supported, on ` export default function Decl(): number { ` |
| `es6/modules/defaultExportsCannotMerge02.ts` | not supported | `export default` is not supported, on ` export default class Decl { ` |
| `es6/modules/defaultExportsCannotMerge03.ts` | not supported | `export default` is not supported, on ` export default class Decl { ` |
| `es6/modules/defaultExportsCannotMerge04.ts` | not supported | `export default` is not supported, on ` export default function Foo(): void { ` |
| `es6/modules/defaultExportWithOverloads01.ts` | not supported | `export default` is not supported, on ` export default function f(); ` |
| `es6/modules/exportAndImport-es5-amd.ts` | not supported | `export default` is not supported, on ` export default function f1(): void { ` |
| `es6/modules/exportAndImport-es5.ts` | not supported | `export default` is not supported, on ` export default function f1(): void { ` |
| `es6/modules/exportBinding.ts` | not supported | `export default` is not supported, on ` export default x; ` |
| `es6/modules/exportsAndImports1-amd.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum D { ` |
| `es6/modules/exportsAndImports1-es6.ts` | duplicate | of `es6/modules/exportsAndImports1-amd.ts` |
| `es6/modules/exportsAndImports1.ts` | duplicate | of `es6/modules/exportsAndImports1-amd.ts` |
| `es6/modules/exportsAndImports2-amd.ts` | not supported | module `t3` does not export `x`, on ` export { x as y, y as x }; ` |
| `es6/modules/exportsAndImports2-es6.ts` | not supported | module `t3` does not export `x`, on ` export { x as y, y as x }; ` |
| `es6/modules/exportsAndImports2.ts` | not supported | module `t3` does not export `x`, on ` export { x as y, y as x }; ` |
| `es6/modules/exportsAndImports3-amd.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` export const enum D { ` |
| `es6/modules/exportsAndImports3-es6.ts` | duplicate | of `es6/modules/exportsAndImports3-amd.ts` |
| `es6/modules/exportsAndImports3.ts` | duplicate | of `es6/modules/exportsAndImports3-amd.ts` |
| `es6/modules/exportsAndImports4-amd.ts` | not supported | `export default` is not supported, on ` export default "hello"; ` |
| `es6/modules/exportsAndImports4-es6.ts` | not supported | `export default` is not supported, on ` export default "hello"; ` |
| `es6/modules/exportsAndImports4.ts` | not supported | `export default` is not supported, on ` export default "hello"; ` |
| `es6/modules/exportsAndImportsWithContextualKeywordNames01.ts` | not supported | expected `,` or `}`, on ` set foo(x: number) { ` |
| `es6/modules/exportsAndImportsWithContextualKeywordNames02.ts` | not supported | `return` is a reserved keyword and can't be used as a name, on ` export { as as return, as }; ` |
| `es6/modules/exportsAndImportsWithUnderscores1.ts` | not supported | `any` is not supported, on ` let R: any = null as unknown as (any); ` |
| `es6/modules/exportsAndImportsWithUnderscores2.ts` | not supported | `any` is not supported, on ` let R: any = null as unknown as (any); ` |
| `es6/modules/exportsAndImportsWithUnderscores3.ts` | not supported | `any` is not supported, on ` let R: any = null as unknown as (any); ` |
| `es6/modules/exportStar.ts` | duplicate | of `es6/modules/exportStar-amd.ts` |
| `es6/modules/importEmptyFromModuleNotExisted.ts` | not supported | empty import specifier list, on ` import {} from 'module-not-existed' ` |
| `es6/modules/multipleDefaultExports01.ts` | not supported | `export default` is not supported, on ` export default class foo { ` |
| `es6/modules/multipleDefaultExports02.ts` | not supported | `export default` is not supported, on ` export default function foo(): void { ` |
| `es6/modules/multipleDefaultExports03.ts` | not supported | `export default` is not supported, on ` export default class C { ` |
| `es6/modules/multipleDefaultExports04.ts` | not supported | `export default` is not supported, on ` export default function f(): void { ` |
| `es6/modules/multipleDefaultExports05.ts` | not supported | `export default` is not supported, on ` export default class AA1 {} ` |
| `es6/modules/reExportDefaultExport.ts` | not supported | `export default` is not supported, on ` export default function f(): void { ` |
| `es6/newTarget/invalidNewTarget.es5.ts` | not supported | expected expression, on ` const a = new.target; ` |
| `es6/newTarget/invalidNewTarget.es6.ts` | not supported | expected expression, on ` const a = new.target; ` |
| `es6/newTarget/newTarget.es5.ts` | not supported | expected expression, on ` const a = new.target; ` |
| `es6/newTarget/newTarget.es6.ts` | not supported | expected expression, on ` const a = new.target; ` |
| `es6/newTarget/newTargetNarrowing.ts` | not supported | expected expression, on ` if (new.target.marked === true) { ` |
| `es6/restParameters/emitRestParametersFunction.ts` | not supported | rest parameter requires a type annotation, on ` function bar(...rest): void { } ` |
| `es6/restParameters/emitRestParametersFunctionES6.ts` | not supported | rest parameter requires a type annotation, on ` function bar(...rest): void { } ` |
| `es6/restParameters/emitRestParametersFunctionExpression.ts` | not supported | parameter `rest` requires a type annotation, on ` let funcExp3 = (function (...rest) { })() ` |
| `es6/restParameters/emitRestParametersFunctionExpressionES6.ts` | not supported | parameter `rest` requires a type annotation, on ` let funcExp3 = (function (...rest) { })() ` |
| `es6/restParameters/emitRestParametersFunctionProperty.ts` | not supported | rest parameter requires a type annotation, on ` func1: (...rest) => void ` |
| `es6/restParameters/emitRestParametersFunctionPropertyES6.ts` | not supported | rest parameter requires a type annotation, on ` func1: (...rest) => void ` |
| `es6/restParameters/emitRestParametersMethod.ts` | not supported | rest parameter requires a type annotation, on ` constructor(name: string, ...rest) { } ` |
| `es6/restParameters/emitRestParametersMethodES6.ts` | not supported | rest parameter requires a type annotation, on ` constructor(name: string, ...rest) { } ` |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandProperties.ts` | not supported | `let` declaration requires an initializer, on ` let a, b, c; ` |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesErrorFromNoneExistingIdentifier.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448 |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesErrorFromNotUsingIdentifier.ts` | not supported | expected `:` after field name, on ` "stringLiteral", ` |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesErrorWithModule.ts` | not supported | expected `;` after expression, on ` namespace m { ` |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesES6.ts` | not supported | `let` declaration requires an initializer, on ` let a, b, c; ` |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesFunctionArgument.ts` | the port changes what it checks | `tsc` then reports TS2451, TS2345 |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesFunctionArgument2.ts` | the port changes what it checks | `tsc` then reports TS2451 |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesWithModule.ts` | not supported | expected `;` after expression, on ` namespace m { ` |
| `es6/shorthandPropertyAssignment/objectLiteralShorthandPropertiesWithModuleES6.ts` | not supported | expected `;` after expression, on ` namespace m { ` |
| `es6/spread/arrayLiteralSpreadES5iterable.ts` | duplicate | of `es6/spread/arrayLiteralSpread.ts` |
| `es6/spread/arraySpreadImportHelpers.ts` | not supported | declaration files (`.d.ts`) |
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
| `es6/templates/taggedTemplateStringsTypeArgumentInference.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `es6/templates/taggedTemplateStringsTypeArgumentInferenceES6.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `es6/templates/taggedTemplateStringsWithIncompatibleTypedTags.ts` | not supported | index signatures require string keys; use `[key: string]: V`, on ` [x: number]: I; ` |
| `es6/templates/taggedTemplateStringsWithIncompatibleTypedTagsES6.ts` | not supported | index signatures require string keys; use `[key: string]: V`, on ` [x: number]: I; ` |
| `es6/templates/taggedTemplateStringsWithManyCallAndMemberExpressions.ts` | not supported | construct signatures |
| `es6/templates/taggedTemplateStringsWithManyCallAndMemberExpressionsES6.ts` | not supported | construct signatures |
| `es6/templates/taggedTemplateStringsWithOverloadResolution1_ES6.ts` | not supported | expected `{`, on ` function foo(strs: TemplateStringsArray): number; ` |
| `es6/templates/taggedTemplateStringsWithOverloadResolution1.ts` | not supported | expected `{`, on ` function foo(strs: TemplateStringsArray): number; ` |
| `es6/templates/taggedTemplateStringsWithOverloadResolution2_ES6.ts` | not supported | expected `{`, on ` function foo1(strs: TemplateStringsArray, x: number): string; ` |
| `es6/templates/taggedTemplateStringsWithOverloadResolution2.ts` | not supported | expected `{`, on ` function foo1(strs: TemplateStringsArray, x: number): string; ` |
| `es6/templates/taggedTemplateStringsWithOverloadResolution3_ES6.ts` | not supported | expected `{`, on ` function fn1(strs: TemplateStringsArray, s: string): string; ` |
| `es6/templates/taggedTemplateStringsWithOverloadResolution3.ts` | not supported | expected `{`, on ` function fn1(strs: TemplateStringsArray, s: string): string; ` |
| `es6/templates/taggedTemplateStringsWithTagNamedDeclare.ts` | not supported | `any` is not supported, on ` function declare(x: any, ...ys: any[]): void { ` |
| `es6/templates/taggedTemplateStringsWithTagNamedDeclareES6.ts` | not supported | `any` is not supported, on ` function declare(x: any, ...ys: any[]): void { ` |
| `es6/templates/taggedTemplateStringsWithTagsTypedAsAny.ts` | not supported | `any` is not supported, on ` let f: any = null as unknown as (any); ` |
| `es6/templates/taggedTemplateStringsWithTagsTypedAsAnyES6.ts` | not supported | `any` is not supported, on ` let f: any = null as unknown as (any); ` |
| `es6/templates/taggedTemplateStringsWithTypedTags.ts` | not supported | index signatures require string keys; use `[key: string]: V`, on ` [x: number]: I; ` |
| `es6/templates/taggedTemplateStringsWithTypedTagsES6.ts` | not supported | index signatures require string keys; use `[key: string]: V`, on ` [x: number]: I; ` |
| `es6/templates/taggedTemplateStringsWithTypeErrorInFunctionExpressionsInSubstitutionExpression.ts` | not supported | `any` is not supported, on ` function foo(...rest: any[]): void { ` |
| `es6/templates/taggedTemplateStringsWithTypeErrorInFunctionExpressionsInSubstitutionExpressionES6.ts` | not supported | `any` is not supported, on ` function foo(...rest: any[]): void { ` |
| `es6/templates/taggedTemplatesWithTypeArguments1.ts` | not supported | `any` is not supported, on ` function f<T>(strs: TemplateStringsArray, ...callbacks: Array<(x: T) => any>)... ` |
| `es6/templates/taggedTemplatesWithTypeArguments2.ts` | not supported | construct signatures |
| `es6/templates/taggedTemplateUntypedTagCall01.ts` | not supported | expected `;` after expression, on `` tag `Hello world!`; `` |
| `es6/templates/taggedTemplateWithConstructableTag01.ts` | not supported | expected `;` after expression, on `` CtorTag `Hello world!`; `` |
| `es6/templates/taggedTemplateWithConstructableTag02.ts` | not supported | construct signatures |
| `es6/templates/TemplateExpression1.ts` | not supported | expected `}` to close template interpolation, on ` function main(): void {} ` |
| `es6/templates/templateStringBinaryOperationsES6.ts` | duplicate | of `es6/templates/templateStringBinaryOperations.ts` |
| `es6/templates/templateStringBinaryOperationsInvalid.ts` | duplicate | of `es6/templates/templateStringBinaryOperationsES6Invalid.ts` |
| `es6/templates/templateStringControlCharacterEscapes01_ES6.ts` | not supported | unknown escape sequence `\x`, on `` let x = `\0\x00\u0000 0 00 0000`; `` |
| `es6/templates/templateStringControlCharacterEscapes01.ts` | not supported | unknown escape sequence `\x`, on `` let x = `\0\x00\u0000 0 00 0000`; `` |
| `es6/templates/templateStringControlCharacterEscapes02_ES6.ts` | not supported | unknown escape sequence `\x`, on `` let x = `\x19\u0019 19`; `` |
| `es6/templates/templateStringControlCharacterEscapes02.ts` | not supported | unknown escape sequence `\x`, on `` let x = `\x19\u0019 19`; `` |
| `es6/templates/templateStringControlCharacterEscapes03_ES6.ts` | not supported | unknown escape sequence `\x`, on `` let x = `\x1F\u001f 1F 1f`; `` |
| `es6/templates/templateStringControlCharacterEscapes03.ts` | not supported | unknown escape sequence `\x`, on `` let x = `\x1F\u001f 1F 1f`; `` |
| `es6/templates/templateStringControlCharacterEscapes04_ES6.ts` | not supported | unknown escape sequence `\x`, on `` let x = `\x20\u0020 20`; `` |
| `es6/templates/templateStringControlCharacterEscapes04.ts` | not supported | unknown escape sequence `\x`, on `` let x = `\x20\u0020 20`; `` |
| `es6/templates/templateStringInArrowFunctionES6.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `es6/templates/templateStringInCallExpressionES6.ts` | duplicate | of `es6/templates/templateStringInCallExpression.ts` |
| `es6/templates/templateStringInConditionalES6.ts` | duplicate | of `es6/templates/templateStringInConditional.ts` |
| `es6/templates/templateStringInDeleteExpression.ts` | not supported | `delete` |
| `es6/templates/templateStringInDeleteExpressionES6.ts` | not supported | `delete` |
| `es6/templates/templateStringInEqualityChecksES6.ts` | duplicate | of `es6/templates/templateStringInEqualityChecks.ts` |
| `es6/templates/templateStringInFunctionExpressionES6.ts` | duplicate | of `es6/templates/templateStringInFunctionExpression.ts` |
| `es6/templates/templateStringInFunctionParameterType.ts` | not supported | expected parameter name, on `` function f(`hello`); `` |
| `es6/templates/templateStringInFunctionParameterTypeES6.ts` | not supported | expected parameter name, on `` function f(`hello`); `` |
| `es6/templates/templateStringInIndexExpressionES6.ts` | duplicate | of `es6/templates/templateStringInIndexExpression.ts` |
| `es6/templates/templateStringInInOperatorES6.ts` | duplicate | of `es6/templates/templateStringInInOperator.ts` |
| `es6/templates/templateStringInInstanceOfES6.ts` | duplicate | of `es6/templates/templateStringInInstanceOf.ts` |
| `es6/templates/templateStringInModuleName.ts` | not supported | expected `;` after expression, on `` declare module `M1` { `` |
| `es6/templates/templateStringInModuleNameES6.ts` | not supported | expected `;` after expression, on `` declare module `M1` { `` |
| `es6/templates/templateStringInModuloES6.ts` | duplicate | of `es6/templates/templateStringInModulo.ts` |
| `es6/templates/templateStringInMultiplicationES6.ts` | duplicate | of `es6/templates/templateStringInMultiplication.ts` |
| `es6/templates/templateStringInNewExpressionES6.ts` | duplicate | of `es6/templates/templateStringInNewExpression.ts` |
| `es6/templates/templateStringInNewOperator.ts` | not supported | expected `(` after constructor name in `new` expression, on `` let x = new `abc${ 1 }def`; `` |
| `es6/templates/templateStringInNewOperatorES6.ts` | not supported | expected `(` after constructor name in `new` expression, on `` let x = new `abc${ 1 }def`; `` |
| `es6/templates/templateStringInObjectLiteral.ts` | not supported | expected field name, on `` `b`: 321 `` |
| `es6/templates/templateStringInObjectLiteralES6.ts` | not supported | expected field name, on `` `b`: 321 `` |
| `es6/templates/templateStringInParenthesesES6.ts` | duplicate | of `es6/templates/templateStringInParentheses.ts` |
| `es6/templates/templateStringInPropertyAssignmentES6.ts` | duplicate | of `es6/templates/templateStringInPropertyAssignment.ts` |
| `es6/templates/templateStringInPropertyName1.ts` | not supported | expected field name, on `` `a`: 321 `` |
| `es6/templates/templateStringInPropertyName2.ts` | not supported | expected field name, on `` `abc${ 123 }def${ 456 }ghi`: 321 `` |
| `es6/templates/templateStringInPropertyNameES6_1.ts` | not supported | expected field name, on `` `a`: 321 `` |
| `es6/templates/templateStringInPropertyNameES6_2.ts` | not supported | expected field name, on `` `abc${ 123 }def${ 456 }ghi`: 321 `` |
| `es6/templates/templateStringInSwitchAndCaseES6.ts` | duplicate | of `es6/templates/templateStringInSwitchAndCase.ts` |
| `es6/templates/templateStringInTaggedTemplate.ts` | not supported | expected `;` after expression, on `` `I AM THE ${ `${ `TAG` } ` } PORTION`    `I ${ "AM" } THE TEMPLATE PORTION` `` |
| `es6/templates/templateStringInTaggedTemplateES6.ts` | not supported | expected `;` after expression, on `` `I AM THE ${ `${ `TAG` } ` } PORTION`    `I ${ "AM" } THE TEMPLATE PORTION` `` |
| `es6/templates/templateStringInTypeAssertion.ts` | not supported | `any` is not supported, on `` let x = <any>`abc${ 123 }def`; `` |
| `es6/templates/templateStringInTypeAssertionES6.ts` | not supported | `any` is not supported, on `` let x = <any>`abc${ 123 }def`; `` |
| `es6/templates/templateStringInTypeOfES6.ts` | duplicate | of `es6/templates/templateStringInTypeOf.ts` |
| `es6/templates/templateStringInUnaryPlusES6.ts` | duplicate | of `es6/templates/templateStringInUnaryPlus.ts` |
| `es6/templates/templateStringInWhileES6.ts` | duplicate | of `es6/templates/templateStringInWhile.ts` |
| `es6/templates/templateStringInYieldKeyword.ts` | not supported | expected function name, on ` function* gen(): Generator<string, void, unknown> { ` |
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
| `es6/templates/templateStringWithEmbeddedAdditionES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedAddition.ts` |
| `es6/templates/templateStringWithEmbeddedArrayES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedArray.ts` |
| `es6/templates/templateStringWithEmbeddedArrowFunction.ts` | not supported | template-literal interpolation: `.toString()` not supported on `(arg0: <error>) => <error>`, on `` let x = `abc${ x => x }def`; `` |
| `es6/templates/templateStringWithEmbeddedArrowFunctionES6.ts` | not supported | template-literal interpolation: `.toString()` not supported on `(arg0: <error>) => <error>`, on `` let x = `abc${ x => x }def`; `` |
| `es6/templates/templateStringWithEmbeddedCommentsES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedComments.ts` |
| `es6/templates/templateStringWithEmbeddedConditionalES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedConditional.ts` |
| `es6/templates/templateStringWithEmbeddedDivisionES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedDivision.ts` |
| `es6/templates/templateStringWithEmbeddedFunctionExpression.ts` | not supported | template-literal interpolation: `.toString()` not supported on `() => () => unknown`, on `` let x = `abc${ function y() { return y; } }def`; `` |
| `es6/templates/templateStringWithEmbeddedFunctionExpressionES6.ts` | not supported | template-literal interpolation: `.toString()` not supported on `() => () => unknown`, on `` let x = `abc${ function y() { return y; } }def`; `` |
| `es6/templates/templateStringWithEmbeddedInOperatorES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedInOperator.ts` |
| `es6/templates/templateStringWithEmbeddedInstanceOfES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedInstanceOf.ts` |
| `es6/templates/templateStringWithEmbeddedModuloES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedModulo.ts` |
| `es6/templates/templateStringWithEmbeddedMultiplicationES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedMultiplication.ts` |
| `es6/templates/templateStringWithEmbeddedNewOperator.ts` | not supported | `new` expects a constructor; `StringConstructor` declares no `new` method, on `` let x = `abc${ new String("Hi") }def`; `` |
| `es6/templates/templateStringWithEmbeddedNewOperatorES6.ts` | not supported | `new` expects a constructor; `StringConstructor` declares no `new` method, on `` let x = `abc${ new String("Hi") }def`; `` |
| `es6/templates/templateStringWithEmbeddedObjectLiteralES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedObjectLiteral.ts` |
| `es6/templates/templateStringWithEmbeddedTemplateStringES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedTemplateString.ts` |
| `es6/templates/templateStringWithEmbeddedTypeAssertionOnAddition.ts` | not supported | `any` is not supported, on `` let x = `abc${ <any>(10 + 10) }def`; `` |
| `es6/templates/templateStringWithEmbeddedTypeAssertionOnAdditionES6.ts` | not supported | `any` is not supported, on `` let x = `abc${ <any>(10 + 10) }def`; `` |
| `es6/templates/templateStringWithEmbeddedTypeOfOperatorES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedTypeOfOperator.ts` |
| `es6/templates/templateStringWithEmbeddedUnaryPlusES6.ts` | duplicate | of `es6/templates/templateStringWithEmbeddedUnaryPlus.ts` |
| `es6/templates/templateStringWithEmbeddedYieldKeyword.ts` | not supported | expected function name, on ` function* gen: Generator<number, void, unknown> { ` |
| `es6/templates/templateStringWithEmbeddedYieldKeywordES6.ts` | not supported | expected function name, on ` function* gen(): Generator<number, void, unknown> { ` |
| `es6/templates/templateStringWithEmptyLiteralPortionsES6.ts` | duplicate | of `es6/templates/templateStringWithEmptyLiteralPortions.ts` |
| `es6/templates/templateStringWithOpenCommentInStringPortionES6.ts` | duplicate | of `es6/templates/templateStringWithOpenCommentInStringPortion.ts` |
| `es6/templates/templateStringWithPropertyAccessES6.ts` | duplicate | of `es6/templates/templateStringWithPropertyAccess.ts` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions10.ts` | checks too little | 0 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInRegularExpressions11.ts` | checks too little | 0 after the port |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings04.ts` | not supported | invalid code point in `\u{…}`: too many digits, on ` let x = "\u{00000000}"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings07.ts` | not supported | invalid code point in `\u{…}`: exceeds U+10FFFF, on ` let x = "\u{110000}"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings12.ts` | not supported | invalid code point in `\u{…}`: too many digits, on ` let x = "\u{FFFFFFFF}"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings14.ts` | not supported | invalid unicode escape: expected hex digits, on ` let x = "\u{-DDDD}"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings17.ts` | not supported | invalid unicode escape: expected hex digits, on ` let x = "\u{r}\u{n}\u{t}"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings19.ts` | not supported | invalid unicode escape: expected hex digits, on ` let x = "\u{}"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings20.ts` | not supported | invalid unicode escape: expected hex digits, on ` let x = "\u{"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings21.ts` | not supported | invalid unicode escape: expected `}`, on ` let x = "\u{67"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings22.ts` | not supported | invalid unicode escape: expected `}`, on ` let x = "\u{00000000000067"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings23.ts` | not supported | invalid code point in `\u{…}`: too many digits, on ` let x = "\u{00000000000067}"; ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings24.ts` | not supported | invalid unicode escape: expected `}`, on ` let x = "\u{00000000000067 ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings25.ts` | not supported | invalid code point in `\u{…}`: too many digits, on ` let x = "\u{00000000000067} ` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates04.ts` | not supported | invalid code point in `\u{…}`: too many digits, on `` let x = `\u{00000000}`; `` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates07.ts` | not supported | invalid code point in `\u{…}`: exceeds U+10FFFF, on `` let x = `\u{110000}`; `` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates12.ts` | not supported | invalid code point in `\u{…}`: too many digits, on `` let x = `\u{FFFFFFFF}`; `` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates14.ts` | not supported | invalid unicode escape: expected hex digits, on `` let x = `\u{-DDDD}`; `` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates17.ts` | not supported | invalid unicode escape: expected hex digits, on `` let x = `\u{r}\u{n}\u{t}`; `` |
| `es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates19.ts` | not supported | invalid unicode escape: expected hex digits, on `` let x = `\u{}`; `` |
| `es6/variableDeclarations/VariableDeclaration1_es6.ts` | not supported | expected identifier after `let`/`const`, on ` function main(): void {} ` |
| `es6/variableDeclarations/VariableDeclaration11_es6.ts` | not supported | expected identifier after `let`/`const`, on ` function main(): void {} ` |
| `es6/variableDeclarations/VariableDeclaration12_es6.ts` | not supported | expected identifier after `let`/`const`, on ` x ` |
| `es6/variableDeclarations/VariableDeclaration13_es6.ts` | not supported | `let` is a reserved keyword and can't be used as a name, on ` let let: any = null as unknown as (any); ` |
| `es6/variableDeclarations/VariableDeclaration2_es6.ts` | not supported | `const` declaration requires an initializer, on ` function main(): void {} ` |
| `es6/variableDeclarations/VariableDeclaration6_es6.ts` | not supported | expected identifier after `let`/`const`, on ` function main(): void {} ` |
| `es6/variableDeclarations/VariableDeclaration7_es6.ts` | not supported | `let` declaration requires an initializer, on ` function main(): void {} ` |
| `es6/yieldExpressions/` | not supported | generators |
| `es7/exponentiationOperator/compoundExponentiationAssignmentLHSIsReference.ts` | not supported | `any` is not supported, on ` let value: any = null as unknown as (any); ` |
| `es7/exponentiationOperator/compoundExponentiationAssignmentLHSIsValue.ts` | not supported | `any` is not supported, on ` let value: any = null as unknown as (any); ` |
| `es7/exponentiationOperator/emitCompoundExponentiationAssignmentWithIndexingOnLHS2.ts` | not supported | expected field name in object type, on ` function foo(): { 0: number; } { ` |
| `es7/exponentiationOperator/emitCompoundExponentiationAssignmentWithIndexingOnLHS3.ts` | not supported | expected `,` or `}`, on ` get 0() { ` |
| `es7/exponentiationOperator/emitExponentiationOperator4.ts` | not supported | `any` is not supported, on ` let temp: any = null as unknown as (any); ` |
| `es7/exponentiationOperator/exponentiationOperatorSyntaxError2.ts` | not supported | `any` is not supported, on ` let temp: any = null as unknown as (any); ` |
| `es7/exponentiationOperator/exponentiationOperatorWithAnyAndNumber.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `es7/exponentiationOperator/exponentiationOperatorWithInvalidSimpleUnaryExpressionOperands.ts` | not supported | `any` is not supported, on ` let temp: any = null as unknown as (any); ` |
| `es7/exponentiationOperator/exponentiationOperatorWithNew.ts` | not supported | construct signatures |
| `es7/exponentiationOperator/exponentiationOperatorWithTypeParameter.ts` | not supported | `let` declaration requires an initializer, on ` let a!: any = null as unknown as (any); ` |
| `es7/trailingCommasInBindingPatterns.ts` | not supported | expected expression, on ` const {...b,} = {}; ` |
| `es7/trailingCommasInGetter.ts` | not supported | expected parameter name, on ` get x(,): number { return 0; } ` |
| `esDecorators/` | not supported | decorators |
| `esnext/esnextSharedMemory.ts` | the port changes what it checks | `tsc` then reports TS2550 |
| `esnext/logicalAssignment/logicalAssignment11.ts` | not supported | expected expression, on ` e ??= x ?? "x" ` |
| `expressions/asOperator/asOpEmitParens.ts` | not supported | construct signatures |
| `expressions/asOperator/asOperator4.ts` | not supported | `any` is not supported, on ` <any>foo; ` |
| `expressions/asOperator/asOperatorAmbiguity.ts` | not supported | `any` is not supported, on ` let x: any = null as unknown as (any); ` |
| `expressions/asOperator/asOperatorASI.ts` | not supported | `any` is not supported, on ` function as(...args: any[]): void { } ` |
| `expressions/assignmentOperator/assignmentGenericLookupTypeNarrowing.ts` | not supported | expected `:` after index parameter name, on ` let mappedObject: {[K in "foo"]: null \| {x: string}} = {foo: {x: "hello"}}; ` |
| `expressions/assignmentOperator/assignmentLHSIsReference.ts` | not supported | `any` is not supported, on ` let value: any = null as unknown as (any); ` |
| `expressions/assignmentOperator/assignmentLHSIsValue.ts` | not supported | `any` is not supported, on ` let value: any = null as unknown as (any); ` |
| `expressions/assignmentOperator/assignmentTypeNarrowing.ts` | not supported | invalid assignment target, on ` [x] = [true]; ` |
| `expressions/assignmentOperator/compoundAssignmentLHSIsReference.ts` | not supported | `let` declaration requires an initializer, on ` let value; ` |
| `expressions/assignmentOperator/compoundAssignmentLHSIsValue.ts` | not supported | `any` is not supported, on ` let value: any = null as unknown as (any); ` |
| `expressions/binaryOperators/additionOperator/additionOperatorWithAnyAndEveryType.ts` | not supported | expected `;` after expression, on ` namespace M { export let a } ` |
| `expressions/binaryOperators/additionOperator/additionOperatorWithConstrainedTypeParameter.ts` | not supported | expected `,` or `>`, on ` function sum<T extends Record<K, number>, K extends string>(n: number, v: T, ... ` |
| `expressions/binaryOperators/arithmeticOperator/arithmeticOperatorWithTypeParameter.ts` | not supported | `let` declaration requires an initializer, on ` let a!: any = null as unknown as (any); ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithIdenticalObjects.ts` | not supported | construct signatures |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithIntersectionType.ts` | not supported | intersection types |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNoRelationshipObjectsOnCallSignature.ts` | not supported | construct signatures |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNoRelationshipObjectsOnConstructorSignature.ts` | not supported | construct signatures |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNoRelationshipObjectsOnIndexSignature.ts` | not supported | index signatures |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNoRelationshipObjectsOnInstantiatedCallSignature.ts` | not supported | expected `:` after field name, on ` let a1: { fn<T>(x: T): T } = null as unknown as ({ fn<T>(x: T): T }); ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNoRelationshipObjectsOnInstantiatedConstructorSignature.ts` | not supported | construct signatures |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNoRelationshipObjectsOnProperty.ts` | not supported | `as` to `A1` is not yet supported: class types aren't yet supported as `as` targets, on ` let a1: A1 = null as unknown as (A1); ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNumberOperand.ts` | not supported | unknown type `Promise`, on ` const t1: number \| Promise<number> = null as unknown as (number \| Promise<num... ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithNumericLiteral.ts` | not supported | expected `;` after type alias body, on ` type BrandedNum = number & { __numberBrand: any }; ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithOneOperandIsAny.ts` | not supported | `any` is not supported, on ` let x: any = null as unknown as (any); ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithSubtypeObjectOnConstructorSignature.ts` | not supported | construct signatures |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithSubtypeObjectOnIndexSignature.ts` | not supported | index signatures |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithSubtypeObjectOnInstantiatedCallSignature.ts` | not supported | expected `:` after field name, on ` let a1: { fn<T>(x: T): T } = null as unknown as ({ fn<T>(x: T): T }); ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithSubtypeObjectOnInstantiatedConstructorSignature.ts` | not supported | construct signatures |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithSubtypeObjectOnProperty.ts` | not supported | expected `:` and a type for the class field, on ` private a; ` |
| `expressions/binaryOperators/comparisonOperator/comparisonOperatorWithTwoOperandsAreAny.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `expressions/binaryOperators/inOperator/` | not supported | the `in` operator |
| `expressions/binaryOperators/instanceofOperator/instanceofOperatorWithAny.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `expressions/binaryOperators/instanceofOperator/instanceofOperatorWithInvalidOperands.es2015.ts` | not supported | `any` is not supported, on ` let x: any = null as unknown as (any); ` |
| `expressions/binaryOperators/instanceofOperator/instanceofOperatorWithInvalidOperands.ts` | not supported | `any` is not supported, on ` let x: any = null as unknown as (any); ` |
| `expressions/binaryOperators/instanceofOperator/instanceofOperatorWithInvalidStaticToString.ts` | not supported | expected `;` after expression, on ` declare class StaticToString { ` |
| `expressions/binaryOperators/instanceofOperator/instanceofOperatorWithLHSIsObject.ts` | not supported | `any` is not supported, on ` let x1: any = null as unknown as (any); ` |
| `expressions/binaryOperators/instanceofOperator/instanceofOperatorWithLHSIsTypeParameter.ts` | not supported | `any` is not supported, on ` let x: any = null as unknown as (any); ` |
| `expressions/binaryOperators/instanceofOperator/instanceofOperatorWithRHSHasSymbolHasInstance.ts` | not supported | `Symbol` |
| `expressions/binaryOperators/instanceofOperator/instanceofOperatorWithRHSIsSubtypeOfFunction.ts` | not supported | construct signatures |
| `expressions/binaryOperators/logicalOrOperator/logicalOrExpressionIsNotContextuallyTyped.ts` | not supported | parameter `a` requires a type annotation, on ` let r = a \|\| ((a) => a.toLowerCase()); ` |
| `expressions/commaOperator/` | not supported | the comma operator |
| `expressions/conditonalOperator/conditionalOperatorConditoinIsAnyType.ts` | not supported | `any` is not supported, on ` let condAny: any = null as unknown as (any); ` |
| `expressions/conditonalOperator/conditionalOperatorWithoutIdenticalBCT.ts` | not supported | `any` is not supported, on ` class X { propertyX: any; propertyX1: number; propertyX2: string }; ` |
| `expressions/contextualTyping/functionExpressionContextualTyping3.ts` | not supported | `any` is not supported, on ` f((a: any) => "") ` |
| `expressions/contextualTyping/generatedContextualTyping.ts` | not supported | expected `:` and a type for the class field, on ` class Base { private p; } ` |
| `expressions/contextualTyping/getSetAccessorContextualTyping.ts` | not supported | parameter requires a type annotation, on ` set Y(y) { } ` |
| `expressions/contextualTyping/iterableContextualTyping1.ts` | not supported | parameter `s` requires a type annotation, on ` let iter: Iterable<(x: string) => number> = [s => s.length]; ` |
| `expressions/contextualTyping/objectLiteralContextualTyping.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `expressions/contextualTyping/parenthesizedContexualTyping2.ts` | not supported | expected type, on ` type FuncType = (x: <T>(p: T) => T) => typeof x; ` |
| `expressions/contextualTyping/parenthesizedContexualTyping3.ts` | not supported | expected `{`, on ` function tempFun<T>(tempStrs: TemplateStringsArray, g: (x: T) => T, x: T): T; ` |
| `expressions/contextualTyping/taggedTemplateContextualTyping1.ts` | not supported | expected type, on ` type FuncType = (x: <T>(p: T) => T) => typeof x; ` |
| `expressions/contextualTyping/taggedTemplateContextualTyping2.ts` | not supported | expected type, on ` type FuncType1 = (x: <T>(p: T) => T) => typeof x; ` |
| `expressions/elementAccess/letIdentifierInElementAccess01.ts` | not supported | `let` is a reserved keyword and can't be used as a name, on ` let let: any = {}; ` |
| `expressions/functionCalls/callOverload.ts` | not supported | `any` is not supported, on ` function fn(x: any): void { } ` |
| `expressions/functionCalls/callWithMissingVoidUndefinedUnknownAnyInJs.ts` | not supported | JavaScript |
| `expressions/functionCalls/callWithSpread2.ts` | not supported | expected expression, on ` all(...ns) ` |
| `expressions/functionCalls/callWithSpread3.ts` | not supported | rest elements in tuple types are not supported, on ` const s2_: [string, string, ...string[]] = null as unknown as ([string, strin... ` |
| `expressions/functionCalls/callWithSpread4.ts` | not supported | expected field name in object type, on ` (s1: R, s2: RW, s3: RW, s4: RW, s5: W): Promise<void>; ` |
| `expressions/functionCalls/callWithSpread5.ts` | not supported | expected expression, on ` fn(...nnnu, x) ` |
| `expressions/functionCalls/forgottenNew.ts` | not supported | expected `;` after expression, on ` namespace Tools { ` |
| `expressions/functionCalls/functionCalls.ts` | not supported | `any` is not supported, on ` let anyVar: any = null as unknown as (any); ` |
| `expressions/functionCalls/grammarAmbiguities.ts` | not supported | `any` is not supported, on ` function f(n: any): null { return null; } ` |
| `expressions/functionCalls/newWithSpread.ts` | not supported | construct signatures |
| `expressions/functionCalls/newWithSpreadES5.ts` | not supported | construct signatures |
| `expressions/functionCalls/newWithSpreadES6.ts` | not supported | construct signatures |
| `expressions/functionCalls/overloadResolution.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `expressions/functionCalls/overloadResolutionClassConstructors.ts` | not supported | expected `:` and a type for the class field, on ` private n; ` |
| `expressions/functionCalls/overloadResolutionConstructors.ts` | not supported | construct signatures |
| `expressions/functionCalls/typeArgumentInference.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `expressions/functionCalls/typeArgumentInferenceConstructSignatures.ts` | not supported | construct signatures |
| `expressions/functionCalls/typeArgumentInferenceTransitiveConstraints.ts` | not supported | expected `,` or `>`, on ` function fn<A extends Date, B extends A, C extends B>(a: A, b: B, c: C): A[] { ` |
| `expressions/functionCalls/typeArgumentInferenceWithConstraints.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `expressions/functionCalls/typeArgumentInferenceWithObjectLiteral.ts` | not supported | expected `:` and return type, on ` write(value: T); ` |
| `expressions/functions/arrowFunctionContexts.ts` | not supported | expected `;` after expression, on ` with (window) { ` |
| `expressions/functions/contextuallyTypedIife.ts` | not supported | parameter `jake` requires a type annotation, on ` (jake => { })("build"); ` |
| `expressions/functions/voidParamAssignmentCompatibility.ts` | not supported | `as` to `Obj<void>` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` const o: Obj<void> = null as unknown as (Obj<void>); ` |
| `expressions/identifiers/scopeResolutionIdentifiers.ts` | not supported | expected `;` after expression, on ` namespace M1 { ` |
| `expressions/literals/strictModeOctalLiterals.ts` | not supported | expected `,` or `}` after enum member, on ` A = 12 + 01 ` |
| `expressions/newOperator/newOperatorConformance.ts` | not supported | construct signatures |
| `expressions/newOperator/newOperatorErrorCases_noImplicitAny.ts` | not supported | `this` is a reserved keyword and can't be used as a name, on ` function fnNumber(this: void): number { return 90; } ` |
| `expressions/newOperator/newOperatorErrorCases.ts` | not supported | construct signatures |
| `expressions/nullishCoalescingOperator/nullishCoalescingAssignmentVsPrivateFieldsJsEmit1.ts` | not supported | unexpected character `#`, on ` #privateProp: number \| undefined; ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperator_es2020.ts` | duplicate | of `expressions/nullishCoalescingOperator/nullishCoalescingOperator2.ts` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperator_not_strict.ts` | duplicate | of `expressions/nullishCoalescingOperator/nullishCoalescingOperator2.ts` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperator12.ts` | not supported | `any` is not supported, on ` const obj: { arr: any[] } = { arr: [] }; ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperator9.ts` | not supported | parameter `abc` requires a type annotation, on ` let g = f \|\| (abc => { void abc.toLowerCase() }) ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperatorInAsyncGenerator.ts` | not supported | expected `;` after expression, on ` async function* f(a: { b?: number }): AsyncGenerator<number, void, unknown> { ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperatorInParameterBindingPattern.2.ts` | not supported | expected field name in object pattern, on ` (({ [a() ?? "d"]: c = "" }) => { let a; })(); ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperatorInParameterBindingPattern.ts` | not supported | expected field name in object pattern, on ` (({ [a() ?? "d"]: c = "" }) => {})(); ` |
| `expressions/nullishCoalescingOperator/nullishCoalescingOperatorInParameterInitializer.2.ts` | not supported | `let` declaration requires an initializer, on ` ((b: string = a() ?? "d") => { let a; })(); ` |
| `expressions/objectLiterals/objectLiteralErrors.ts` | not supported | unexpected character `#`, on ` #z: 3 ` |
| `expressions/objectLiterals/objectLiteralGettersAndSetters.ts` | not supported | expected `,` or `}`, on ` let sameName1a = { get 'a'() { return ''; }, set a(n) { let p = n; let p_2: s... ` |
| `expressions/operators/incrementAndDecrement.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `expressions/optionalChaining/callChain/callChain.3.ts` | not supported | expected `:` after field name, on ` const a: { m?<T>(obj: {x: T}): T } \| undefined = null as unknown as ({ m?<T>(... ` |
| `expressions/optionalChaining/callChain/callChain.ts` | not supported | `any` is not supported, on ` const o1: undefined \| ((...args: any[]) => number) = null as unknown as (unde... ` |
| `expressions/optionalChaining/callChain/callChainInference.ts` | not supported | `this` is a reserved keyword and can't be used as a name, on ` foo<T>(this: T, arg: keyof T): void; ` |
| `expressions/optionalChaining/callChain/callChainWithSuper.ts` | checks too little | 0 after the port |
| `expressions/optionalChaining/callChain/parentheses.ts` | not supported | `any` is not supported, on ` const o1: ((...args: any[]) => number) = null as unknown as (((...args: any[]... ` |
| `expressions/optionalChaining/callChain/superMethodCall.ts` | not supported | expected `:` and a type for the class field, on ` async asyncMethod(): Promise<void \| undefined> { ` |
| `expressions/optionalChaining/delete/` | not supported | `delete` |
| `expressions/optionalChaining/elementAccessChain/elementAccessChain.3.ts` | not supported | `any` is not supported, on ` const obj: any = null as unknown as (any); ` |
| `expressions/optionalChaining/optionalChainingInLoop.ts` | not supported | `any` is not supported, on ` const list: any[] = [] ` |
| `expressions/optionalChaining/optionalChainingInParameterBindingPattern.2.ts` | not supported | expected field name in object pattern, on ` (({ [a()?.d]: c = "" }) => { let a; })(); ` |
| `expressions/optionalChaining/optionalChainingInParameterBindingPattern.ts` | not supported | expected field name in object pattern, on ` (({ [a()?.d]: c = "" }) => {})(); ` |
| `expressions/optionalChaining/optionalChainingInParameterInitializer.2.ts` | not supported | `let` declaration requires an initializer, on ` ((b: string \| undefined = a()?.d) => { let a; })(); ` |
| `expressions/optionalChaining/optionalChainingInTypeAssertions.ts` | not supported | `any` is not supported, on ` (foo.m as any)?.(); ` |
| `expressions/optionalChaining/privateIdentifierChain/` | not supported | private `#names` |
| `expressions/optionalChaining/propertyAccessChain/propertyAccessChain.3.ts` | not supported | `any` is not supported, on ` const obj: any = null as unknown as (any); ` |
| `expressions/optionalChaining/taggedTemplateChain/` | not supported | tagged templates |
| `expressions/propertyAccess/propertyAccess.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `expressions/superCalls/errorSuperCalls.ts` | not supported | parameter requires a type annotation, on ` set foo(v) { ` |
| `expressions/superPropertyAccess/errorSuperPropertyAccess.ts` | not supported | class fields require a type annotation, on ` m = super.prototype; ` |
| `expressions/superPropertyAccess/superPropertyAccessNoError.ts` | not supported | expected type, on ` returnThis(): this { ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess1.ts` | not supported | expected class member name, on ` [symbol](): number { ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess2.ts` | not supported | expected class member name, on ` [Symbol.isConcatSpreadable](): number { ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess3.ts` | not supported | expected class member name, on ` [symbol](): number { ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess4.ts` | not supported | expected class member name, on ` [symbol](): any { ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess5.ts` | not supported | `any` is not supported, on ` let symbol: any = null as unknown as (any); ` |
| `expressions/superPropertyAccess/superSymbolIndexedAccess6.ts` | not supported | `any` is not supported, on ` let symbol: any = null as unknown as (any); ` |
| `expressions/thisKeyword/thisInInvalidContexts.ts` | not supported | `any` is not supported, on ` constructor(t: any) { } ` |
| `expressions/thisKeyword/thisInInvalidContextsExternalModule.ts` | not supported | `any` is not supported, on ` constructor(t: any) { } ` |
| `expressions/thisKeyword/typeOfThisGeneral.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `expressions/typeAssertions/constAssertionOnEnum.ts` | not supported | expected type, on ` let foo = Foo.A as const; ` |
| `expressions/typeAssertions/constAssertions.ts` | not supported | expected type, on ` let v1 = 'abc' as const; ` |
| `expressions/typeAssertions/typeAssertions.ts` | not supported | `any` is not supported, on ` function fn2(t: any): void { } ` |
| `expressions/typeGuards/typeGuardFunction.ts` | not supported | `any` is not supported, on ` function isA(p1: any): p1 is A { return null as unknown as (boolean); } ` |
| `expressions/typeGuards/typeGuardFunctionErrors.ts` | not supported | construct signatures |
| `expressions/typeGuards/typeGuardFunctionGenerics.ts` | not supported | parameter requires a type annotation, on ` function isB(p1): p1 is B { return null as unknown as (boolean); } ` |
| `expressions/typeGuards/typeGuardFunctionOfFormThis.ts` | not supported | expected type, on ` isLeader(): this is LeadGuard { ` |
| `expressions/typeGuards/typeGuardFunctionOfFormThisErrors.ts` | not supported | expected type, on ` isLeader(): this is LeadGuard { ` |
| `expressions/typeGuards/typeGuardInClass.ts` | not supported | expected expression, on ` let n = class { ` |
| `expressions/typeGuards/typeGuardIntersectionTypes.ts` | not supported | intersection types |
| `expressions/typeGuards/typeGuardNarrowsPrimitiveIntersection.ts` | not supported | intersection types |
| `expressions/typeGuards/typeGuardOfFormInstanceOf.ts` | not supported | `as` to `C1 \| C2` is not yet supported: class types aren't yet supported as `as` targets, on ` let ctor1: C1 \| C2 = null as unknown as (C1 \| C2); ` |
| `expressions/typeGuards/typeGuardOfFormIsType.ts` | not supported | `any` is not supported, on ` function isC1(x: any): x is C1 { ` |
| `expressions/typeGuards/typeGuardOfFormIsTypeOnInterfaces.ts` | duplicate | of `expressions/typeGuards/typeGuardOfFormInstanceOfOnInterface.ts` |
| `expressions/typeGuards/typeGuardOfFormThisMember.ts` | not supported | expected `;` after expression, on ` namespace Test { ` |
| `expressions/typeGuards/typeGuardOfFormThisMemberErrors.ts` | not supported | expected `;` after expression, on ` namespace Test { ` |
| `expressions/typeGuards/typeGuardOfFormTypeOfEqualEqualHasNoEffect.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `expressions/typeGuards/typeGuardOfFormTypeOfNotEqualHasNoEffect.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `expressions/typeGuards/typeGuardOfFormTypeOfOther.ts` | not supported | `as` to `string \| C` is not yet supported: class types aren't yet supported as `as` targets, on ` let strOrC: string \| C = null as unknown as (string \| C); ` |
| `expressions/typeGuards/typeGuardsInClassAccessors.ts` | not supported | static accessors are not supported, on ` static get s1(): string \| number { ` |
| `expressions/typeGuards/typeGuardsInModule.ts` | not supported | expected `;` after expression, on ` namespace m1 { ` |
| `expressions/typeGuards/typeGuardsObjectMethods.ts` | not supported | expected `,` or `}`, on ` get prop() { ` |
| `expressions/typeGuards/typeGuardsWithAny.ts` | not supported | `any` is not supported, on ` let x: any = { p: 0 }; ` |
| `expressions/typeGuards/typeGuardsWithInstanceOf.ts` | not supported | `let` declaration requires an initializer, on ` let result!: I = null as unknown as (I); ` |
| `expressions/typeGuards/typeGuardsWithInstanceOfByConstructorSignature.ts` | not supported | construct signatures |
| `expressions/typeGuards/typeGuardsWithInstanceOfBySymbolHasInstance.ts` | not supported | `Symbol` |
| `expressions/typeGuards/typePredicateASI.ts` | not supported | `any` is not supported, on ` foo(callback: (a: any, b: any) => void): I ` |
| `expressions/typeGuards/typePredicateOnVariableDeclaration01.ts` | not supported | expected type, on ` let x: this is string = null as unknown as (this is string); ` |
| `expressions/typeGuards/typePredicateOnVariableDeclaration02.ts` | not supported | expected `;` after declaration, on ` let y: z = null as unknown as (z) is number; ` |
| `expressions/typeSatisfaction/` | not supported | `satisfies` |
| `expressions/unaryOperators/bitwiseNotOperator/` | not supported | bitwise operators |
| `expressions/unaryOperators/decrementOperator/decrementOperatorWithAnyOtherType.ts` | not supported | `any` is not supported, on ` let ANY: any = null as unknown as (any); ` |
| `expressions/unaryOperators/decrementOperator/decrementOperatorWithEnumType.ts` | not supported | expected enum member name, on ` enum ENUM1 { A, B, "" }; ` |
| `expressions/unaryOperators/decrementOperator/decrementOperatorWithEnumTypeInvalidOperations.ts` | not supported | expected enum member name, on ` enum ENUM1 { A, B, "" }; ` |
| `expressions/unaryOperators/deleteOperator/` | not supported | `delete` |
| `expressions/unaryOperators/incrementOperator/incrementOperatorWithAnyOtherType.ts` | not supported | `any` is not supported, on ` let ANY: any = null as unknown as (any); ` |
| `expressions/unaryOperators/incrementOperator/incrementOperatorWithAnyOtherTypeInvalidOperations.ts` | not supported | `any` is not supported, on ` let ANY1: any = null as unknown as (any); ` |
| `expressions/unaryOperators/incrementOperator/incrementOperatorWithEnumType.ts` | not supported | expected enum member name, on ` enum ENUM1 { A, B, "" }; ` |
| `expressions/unaryOperators/incrementOperator/incrementOperatorWithEnumTypeInvalidOperations.ts` | not supported | expected enum member name, on ` enum ENUM1 { A, B, "" }; ` |
| `expressions/unaryOperators/incrementOperator/incrementOperatorWithUnsupportedBooleanType.ts` | not supported | expected `:` and a type for the class field, on ` public a!: boolean; ` |
| `expressions/unaryOperators/logicalNotOperator/logicalNotOperatorInvalidOperations.ts` | not supported | expected expression, on ` let BOOLEAN3 =!; ` |
| `expressions/unaryOperators/logicalNotOperator/logicalNotOperatorWithAnyOtherType.ts` | not supported | `any` is not supported, on ` let ANY: any = null as unknown as (any); ` |
| `expressions/unaryOperators/negateOperator/negateOperatorInvalidOperations.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `expressions/unaryOperators/negateOperator/negateOperatorWithAnyOtherType.ts` | not supported | `any` is not supported, on ` let ANY: any = null as unknown as (any); ` |
| `expressions/unaryOperators/negateOperator/negateOperatorWithEnumType.ts` | not supported | expected enum member name, on ` enum ENUM1 { A, B, "" }; ` |
| `expressions/unaryOperators/plusOperator/plusOperatorInvalidOperations.ts` | not supported | `let` declaration requires an initializer, on ` let b; ` |
| `expressions/unaryOperators/plusOperator/plusOperatorWithAnyOtherType.ts` | not supported | `any` is not supported, on ` let ANY: any = null as unknown as (any); ` |
| `expressions/unaryOperators/plusOperator/plusOperatorWithEnumType.ts` | not supported | expected enum member name, on ` enum ENUM1 { A, B, "" }; ` |
| `expressions/unaryOperators/typeofOperator/typeofOperatorInvalidOperations.ts` | not supported | expected `;` after declaration, on ` let ANY = ANY typeof ;    //expect error ` |
| `expressions/unaryOperators/typeofOperator/typeofOperatorWithAnyOtherType.ts` | not supported | `any` is not supported, on ` let ANY: any = null as unknown as (any); ` |
| `expressions/unaryOperators/typeofOperator/typeofOperatorWithEnumType.ts` | not supported | expected enum member name, on ` enum ENUM1 { A, B, "" }; ` |
| `expressions/unaryOperators/voidOperator/voidOperatorInvalidOperations.ts` | not supported | expected `;` after declaration, on ` let ANY = ANY void ;    //expect error ` |
| `expressions/unaryOperators/voidOperator/voidOperatorWithAnyOtherType.ts` | not supported | `any` is not supported, on ` let ANY: any = null as unknown as (any); ` |
| `expressions/unaryOperators/voidOperator/voidOperatorWithEnumType.ts` | not supported | expected enum member name, on ` enum ENUM1 { A, B, "" }; ` |
| `externalModules/amdImportAsPrimaryExpression.ts` | not supported | expected `from` after import specifier list, on ` import foo = require("./foo_0"); ` |
| `externalModules/amdImportNotAsPrimaryExpression.ts` | not supported | expected a declaration after `export`, on ` export namespace M1 { ` |
| `externalModules/asiPreventsParsingAsAmbientExternalModule02.ts` | not supported | expected `;` after expression, on ` namespace container { ` |
| `externalModules/circularReference.ts` | not supported | expected `from` after import specifier list, on ` import foo2 = require('./foo2'); ` |
| `externalModules/commonJSImportAsPrimaryExpression.ts` | not supported | expected `from` after import specifier list, on ` import foo = require("./foo_0"); ` |
| `externalModules/commonJsImportBindingElementNarrowType.ts` | not supported | JavaScript |
| `externalModules/commonJSImportNotAsPrimaryExpression.ts` | duplicate | of `externalModules/amdImportNotAsPrimaryExpression.ts` |
| `externalModules/duplicateExportAssignments.ts` | not supported | expected a declaration after `export`, on ` export = x; ` |
| `externalModules/es6/es6modulekind.ts` | not supported | `export default` is not supported, on ` export default class A ` |
| `externalModules/es6/es6modulekindExportClassNameWithObject.ts` | checks too little | 0 after the port |
| `externalModules/es6/es6modulekindWithES2015Target.ts` | not supported | `export default` is not supported, on ` export default class A ` |
| `externalModules/es6/es6modulekindWithES5Target.ts` | not supported | unexpected character `@`, on ` @foo ` |
| `externalModules/es6/es6modulekindWithES5Target10.ts` | not supported | expected `from` after import specifier list, on ` import i = require("mod"); // Error; ` |
| `externalModules/es6/es6modulekindWithES5Target11.ts` | not supported | unexpected character `@`, on ` @foo ` |
| `externalModules/es6/es6modulekindWithES5Target12.ts` | not supported | expected a declaration after `export`, on ` export namespace C { ` |
| `externalModules/es6/es6modulekindWithES5Target2.ts` | not supported | `export default` is not supported, on ` export default class C { ` |
| `externalModules/es6/es6modulekindWithES5Target3.ts` | not supported | unexpected character `@`, on ` @foo ` |
| `externalModules/es6/es6modulekindWithES5Target4.ts` | not supported | `export default` is not supported, on ` export default E; ` |
| `externalModules/es6/es6modulekindWithES5Target5.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` export const enum E2 { ` |
| `externalModules/es6/es6modulekindWithES5Target6.ts` | not supported | rest parameter requires a type annotation, on ` export function f2(...arg): void { ` |
| `externalModules/es6/es6modulekindWithES5Target7.ts` | not supported | expected a declaration after `export`, on ` export namespace N { ` |
| `externalModules/es6/es6modulekindWithES5Target9.ts` | not supported | expected a declaration after `export`, on ` export * from "mod"; ` |
| `externalModules/esnext/esnextmodulekind.ts` | not supported | `export default` is not supported, on ` export default class A ` |
| `externalModules/esnext/esnextmodulekindWithES2015Target.ts` | not supported | `export default` is not supported, on ` export default class A ` |
| `externalModules/esnext/esnextmodulekindWithES5Target.ts` | duplicate | of `externalModules/es6/es6modulekindWithES5Target.ts` |
| `externalModules/esnext/esnextmodulekindWithES5Target10.ts` | not supported | expected `from` after import specifier list, on ` import i = require("mod"); // Error; ` |
| `externalModules/esnext/esnextmodulekindWithES5Target11.ts` | not supported | unexpected character `@`, on ` @foo ` |
| `externalModules/esnext/esnextmodulekindWithES5Target12.ts` | duplicate | of `externalModules/es6/es6modulekindWithES5Target12.ts` |
| `externalModules/esnext/esnextmodulekindWithES5Target2.ts` | not supported | `export default` is not supported, on ` export default class C { ` |
| `externalModules/esnext/esnextmodulekindWithES5Target3.ts` | not supported | unexpected character `@`, on ` @foo ` |
| `externalModules/esnext/esnextmodulekindWithES5Target4.ts` | not supported | `export default` is not supported, on ` export default E; ` |
| `externalModules/esnext/esnextmodulekindWithES5Target5.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` export const enum E2 { ` |
| `externalModules/esnext/esnextmodulekindWithES5Target6.ts` | not supported | rest parameter requires a type annotation, on ` export function f2(...arg): void { ` |
| `externalModules/esnext/esnextmodulekindWithES5Target7.ts` | not supported | expected a declaration after `export`, on ` export namespace N { ` |
| `externalModules/esnext/esnextmodulekindWithES5Target8.ts` | duplicate | of `externalModules/es6/es6modulekindWithES5Target8.ts` |
| `externalModules/esnext/esnextmodulekindWithES5Target9.ts` | not supported | expected a declaration after `export`, on ` export * from "mod"; ` |
| `externalModules/esnext/exnextmodulekindExportClassNameWithObject.ts` | duplicate | of `externalModules/es6/es6modulekindExportClassNameWithObject.ts` |
| `externalModules/exportAmbientClassNameWithObject.ts` | not supported | expected a declaration after `export`, on ` export declare class Object {} ` |
| `externalModules/exportAssignDottedName.ts` | not supported | expected `from` after import specifier list, on ` import foo1 = require('./foo1'); ` |
| `externalModules/exportAssignImportedIdentifier.ts` | not supported | expected `from` after import specifier list, on ` import foo1 = require('./foo1'); ` |
| `externalModules/exportAssignmentAndDeclaration.ts` | not supported | expected a declaration after `export`, on ` export = C1; ` |
| `externalModules/exportAssignmentCircularModules.ts` | not supported | expected `from` after import specifier list, on ` import foo1 = require('./foo_1'); ` |
| `externalModules/exportAssignmentConstrainedGenericType.ts` | not supported | expected `,` or `>`, on ` class Foo<T extends {a: string; b:number;}>{ ` |
| `externalModules/exportAssignmentGenericType.ts` | not supported | expected a declaration after `export`, on ` export = Foo; ` |
| `externalModules/exportAssignmentMergedInterface.ts` | not supported | expected a declaration after `export`, on ` export = Foo; ` |
| `externalModules/exportAssignmentMergedModule.ts` | not supported | expected `;` after expression, on ` namespace Foo { ` |
| `externalModules/exportAssignmentOfExportNamespaceWithDefault.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/exportAssignmentTopLevelClodule.ts` | not supported | expected `;` after expression, on ` namespace Foo { ` |
| `externalModules/exportAssignmentTopLevelEnumdule.ts` | not supported | expected `;` after expression, on ` namespace foo { ` |
| `externalModules/exportAssignmentTopLevelFundule.ts` | not supported | expected `;` after expression, on ` namespace foo { ` |
| `externalModules/exportAssignmentTopLevelIdentifier.ts` | not supported | expected `;` after expression, on ` namespace Foo { ` |
| `externalModules/exportAssignNonIdentifier.ts` | not supported | expected a declaration after `export`, on ` export = typeof x; // Ok ` |
| `externalModules/exportAssignTypes.ts` | not supported | expected a declaration after `export`, on ` export = x; ` |
| `externalModules/exportClassNameWithObjectAMD.ts` | duplicate | of `externalModules/es6/es6modulekindExportClassNameWithObject.ts` |
| `externalModules/exportClassNameWithObjectCommonJS.ts` | duplicate | of `externalModules/es6/es6modulekindExportClassNameWithObject.ts` |
| `externalModules/exportClassNameWithObjectSystem.ts` | duplicate | of `externalModules/es6/es6modulekindExportClassNameWithObject.ts` |
| `externalModules/exportClassNameWithObjectUMD.ts` | duplicate | of `externalModules/es6/es6modulekindExportClassNameWithObject.ts` |
| `externalModules/exportDeclaredModule.ts` | not supported | expected `;` after expression, on ` declare namespace M1 { ` |
| `externalModules/exportDefaultClassNameWithObject.ts` | not supported | `export default` is not supported, on ` export default class Object {} ` |
| `externalModules/exportNonInitializedVariablesAMD.ts` | not supported | expected identifier after `let`/`const`, on ` let; ` |
| `externalModules/exportNonInitializedVariablesCommonJS.ts` | not supported | expected identifier after `let`/`const`, on ` let; ` |
| `externalModules/exportNonInitializedVariablesES6.ts` | not supported | expected identifier after `let`/`const`, on ` let; ` |
| `externalModules/exportNonInitializedVariablesInIfThenStatementNoCrash1.ts` | not supported | `export default` is not supported, on ` export default cssExports; ` |
| `externalModules/exportNonInitializedVariablesSystem.ts` | not supported | expected identifier after `let`/`const`, on ` let; ` |
| `externalModules/exportNonInitializedVariablesUMD.ts` | not supported | expected identifier after `let`/`const`, on ` let; ` |
| `externalModules/exportNonVisibleType.ts` | not supported | expected a declaration after `export`, on ` export = x; // Should fail, I1 not exported. ` |
| `externalModules/exportTypeMergedWithExportStarAsNamespace.ts` | not supported | expected a declaration after `export`, on ` export * as Something from "./Something" ` |
| `externalModules/globalAugmentationModuleResolution.ts` | not supported | empty export specifier list, on ` export { }; ` |
| `externalModules/importImportOnlyModule.ts` | not supported | expected `from` after import specifier list, on ` import c1 = require('./foo_0'); // Makes this an external module ` |
| `externalModules/importNonExternalModule.ts` | not supported | expected `;` after expression, on ` namespace foo { ` |
| `externalModules/importNonStringLiteral.ts` | not supported | expected `from` after import specifier list, on ` import foo = require(x); // invalid ` |
| `externalModules/importTsBeforeDTs.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/initializersInDeclarations.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/invalidSyntaxNamespaceImportWithAMD.ts` | not supported | expected `as <name>` after `import *`, on ` import * from Zero from "./0" ` |
| `externalModules/invalidSyntaxNamespaceImportWithCommonjs.ts` | not supported | expected `as <name>` after `import *`, on ` import * from Zero from "./0" ` |
| `externalModules/invalidSyntaxNamespaceImportWithSystem.ts` | not supported | expected `as <name>` after `import *`, on ` import * from Zero from "./0" ` |
| `externalModules/moduleResolutionWithExtensions.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/moduleResolutionWithoutExtension1.ts` | not supported | `.mts` and `.cts` files |
| `externalModules/moduleResolutionWithoutExtension2.ts` | not supported | `.mts` and `.cts` files |
| `externalModules/moduleResolutionWithoutExtension3.ts` | not supported | JSX |
| `externalModules/moduleResolutionWithoutExtension4.ts` | not supported | JSX |
| `externalModules/moduleResolutionWithoutExtension5.ts` | not supported | `.mts` and `.cts` files |
| `externalModules/moduleResolutionWithoutExtension6.ts` | not supported | `.mts` and `.cts` files |
| `externalModules/moduleResolutionWithoutExtension7.ts` | not supported | `.mts` and `.cts` files |
| `externalModules/moduleResolutionWithoutExtension8.ts` | not supported | `.mts` and `.cts` files |
| `externalModules/moduleScoping.ts` | not supported | script files sharing one global scope |
| `externalModules/multipleExportDefault1.ts` | not supported | `export default` is not supported, on ` export default function Foo (): void { ` |
| `externalModules/multipleExportDefault2.ts` | not supported | `export default` is not supported, on ` export default { ` |
| `externalModules/multipleExportDefault3.ts` | not supported | `export default` is not supported, on ` export default { ` |
| `externalModules/multipleExportDefault4.ts` | not supported | `export default` is not supported, on ` export default class C { } ` |
| `externalModules/multipleExportDefault5.ts` | not supported | `export default` is not supported, on ` export default function bar(): void { } ` |
| `externalModules/multipleExportDefault6.ts` | not supported | `export default` is not supported, on ` export default { ` |
| `externalModules/nameDelimitedBySlashes.ts` | not supported | expected `from` after import specifier list, on ` import foo = require('./test/foo_0'); ` |
| `externalModules/nameWithFileExtension.ts` | not supported | expected `from` after import specifier list, on ` import foo = require('./foo_0.js'); ` |
| `externalModules/nameWithRelativePaths.ts` | not supported | expected a declaration after `export`, on ` export namespace M2 { ` |
| `externalModules/reexportClassDefinition.ts` | not supported | expected a declaration after `export`, on ` export = x; ` |
| `externalModules/relativePathMustResolve.ts` | not supported | expected `from` after import specifier list, on ` import foo = require('./test/foo'); ` |
| `externalModules/relativePathToDeclarationFile.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/rewriteRelativeImportExtensions/cjsErrors.ts` | not supported | expected a declaration after `export`, on ` export = {}; ` |
| `externalModules/rewriteRelativeImportExtensions/emit.ts` | not supported | JavaScript |
| `externalModules/rewriteRelativeImportExtensions/emitModuleCommonJS.ts` | not supported | JavaScript |
| `externalModules/rewriteRelativeImportExtensions/nodeModulesTsFiles.ts` | not supported | `node_modules` packages |
| `externalModules/rewriteRelativeImportExtensions/nonTSExtensions.ts` | not supported | JSON modules |
| `externalModules/rewriteRelativeImportExtensions/packageJsonImportsErrors.ts` | not supported | JSON modules |
| `externalModules/topLevelAmbientModule.ts` | not supported | script files sharing one global scope |
| `externalModules/topLevelAwait.1.ts` | not supported | async/await |
| `externalModules/topLevelAwait.2.ts` | not supported | async/await |
| `externalModules/topLevelAwait.3.ts` | not supported | async/await |
| `externalModules/topLevelAwaitErrors.1.ts` | not supported | async/await |
| `externalModules/topLevelAwaitErrors.10.ts` | not supported | async/await |
| `externalModules/topLevelAwaitErrors.11.ts` | not supported | async/await |
| `externalModules/topLevelAwaitErrors.12.ts` | not supported | async/await |
| `externalModules/topLevelAwaitErrors.2.ts` | not supported | async/await |
| `externalModules/topLevelAwaitErrors.3.ts` | not supported | async/await |
| `externalModules/topLevelAwaitErrors.4.ts` | not supported | async/await |
| `externalModules/topLevelAwaitErrors.5.ts` | not supported | async/await |
| `externalModules/topLevelAwaitErrors.6.ts` | not supported | async/await |
| `externalModules/topLevelAwaitErrors.7.ts` | not supported | async/await |
| `externalModules/topLevelAwaitErrors.8.ts` | not supported | async/await |
| `externalModules/topLevelAwaitErrors.9.ts` | not supported | async/await |
| `externalModules/topLevelAwaitNonModule.ts` | not supported | async/await |
| `externalModules/topLevelFileModule.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/topLevelFileModuleMissing.ts` | not supported | expected `from` after import specifier list, on ` import foo = require("vs/foo"); ` |
| `externalModules/topLevelModuleDeclarationAndFile.ts` | not supported | script files sharing one global scope |
| `externalModules/typeAndNamespaceExportMerge.ts` | not supported | expected a declaration after `export`, on ` export * as Drink from "./constants"; ` |
| `externalModules/typeOnly/allowsImportingTsExtension.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/typeOnly/ambient.ts` | not supported | expected `:` and a type for the class field, on ` export class A { a!: string } ` |
| `externalModules/typeOnly/chained.ts` | not supported | expected `:` and a type for the class field, on ` class A { a!: string } ` |
| `externalModules/typeOnly/chained2.ts` | not supported | expected `:` and a type for the class field, on ` class A { a!: string } ` |
| `externalModules/typeOnly/circular1.ts` | not supported | circular import: a -> b -> a |
| `externalModules/typeOnly/circular2.ts` | not supported | circular import: a -> b -> a |
| `externalModules/typeOnly/circular3.ts` | not supported | circular import: a -> b -> a |
| `externalModules/typeOnly/circular4.ts` | not supported | expected a declaration after `export`, on ` export namespace ns1 { ` |
| `externalModules/typeOnly/cjsImportInES2015.ts` | not supported | `node_modules` packages |
| `externalModules/typeOnly/computedPropertyName.ts` | not supported | expected `:` after index parameter name, on ` [onInit]?(): void; ` |
| `externalModules/typeOnly/enums.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum SymbolFlags { ` |
| `externalModules/typeOnly/exportDeclaration_missingBraces.ts` | not supported | expected `;` after expression, on ` namespace ns { ` |
| `externalModules/typeOnly/exportDeclaration_moduleSpecifier-isolatedModules.ts` | checks too little | 0 after the port |
| `externalModules/typeOnly/exportDeclaration_moduleSpecifier.ts` | not supported | `as` to `A` is not yet supported: class types aren't yet supported as `as` targets, on ` const a: A = null as unknown as (A); ` |
| `externalModules/typeOnly/exportDeclaration.ts` | not supported | expected a declaration after `export`, on ` export = A; ` |
| `externalModules/typeOnly/exportDefault.ts` | not supported | `export default` is not supported, on ` export default types; ` |
| `externalModules/typeOnly/exportNamespace_js.ts` | not supported | JavaScript |
| `externalModules/typeOnly/exportNamespace1.ts` | not supported | expected a declaration after `export`, on ` export * from './b'; ` |
| `externalModules/typeOnly/exportNamespace10.ts` | not supported | expected a declaration after `export`, on ` export type * as ns from "./a"; ` |
| `externalModules/typeOnly/exportNamespace11.ts` | not supported | expected a declaration after `export`, on ` export type * from './ghost' ` |
| `externalModules/typeOnly/exportNamespace12.ts` | not supported | expected a declaration after `export`, on ` export type * from './values' ` |
| `externalModules/typeOnly/exportNamespace2.ts` | not supported | expected a declaration after `export`, on ` export * as a from './a'; ` |
| `externalModules/typeOnly/exportNamespace3.ts` | not supported | expected a declaration after `export`, on ` export * as a from './b'; ` |
| `externalModules/typeOnly/exportNamespace4.ts` | not supported | expected a declaration after `export`, on ` export type * from './a'; ` |
| `externalModules/typeOnly/exportNamespace5.ts` | not supported | expected a declaration after `export`, on ` export type * from "./a"; ` |
| `externalModules/typeOnly/exportNamespace6.ts` | not supported | expected a declaration after `export`, on ` export type * from "./a"; ` |
| `externalModules/typeOnly/exportNamespace7.ts` | not supported | expected a declaration after `export`, on ` export type * from "./a"; ` |
| `externalModules/typeOnly/exportNamespace8.ts` | not supported | expected a declaration after `export`, on ` export type * from "./a"; ` |
| `externalModules/typeOnly/exportNamespace9.ts` | not supported | expected a declaration after `export`, on ` export type * from "./a"; ` |
| `externalModules/typeOnly/exportSpecifiers_js.ts` | not supported | JavaScript |
| `externalModules/typeOnly/exportSpecifiers.ts` | not supported | expected local name after `as`, on ` export { type as }; ` |
| `externalModules/typeOnly/extendsClause.ts` | not supported | module `ns` does not export `types`, on ` export { types }; ` |
| `externalModules/typeOnly/filterNamespace_import.ts` | not supported | expected `;` after expression, on ` namespace ns { ` |
| `externalModules/typeOnly/generic.ts` | not supported | expected `:` and a type for the class field, on ` export class A<T> { a!: T } ` |
| `externalModules/typeOnly/grammarErrors.ts` | not supported | JavaScript |
| `externalModules/typeOnly/implementsClause.ts` | not supported | module `ns` does not export `types`, on ` export { types }; ` |
| `externalModules/typeOnly/importClause_default.ts` | not supported | `export default` is not supported, on ` export default class A { a!: string } ` |
| `externalModules/typeOnly/importDefaultNamedType.ts` | not supported | `export default` is not supported, on ` export default class A {} ` |
| `externalModules/typeOnly/importDefaultNamedType2.ts` | not supported | `export default` is not supported, on ` export default class A {} ` |
| `externalModules/typeOnly/importDefaultNamedType3.ts` | not supported | expected `from` after import specifier list, on ` import type from = require('./a'); ` |
| `externalModules/typeOnly/importEquals1.ts` | not supported | expected a declaration after `export`, on ` export = types; // Error ` |
| `externalModules/typeOnly/importEquals2.ts` | not supported | expected a declaration after `export`, on ` export = a; ` |
| `externalModules/typeOnly/importEquals3.ts` | not supported | expected `from` after import specifier list, on ` import A = a.A; // Error ` |
| `externalModules/typeOnly/importEqualsDeclaration.ts` | not supported | expected `:` and a type for the class field, on ` class A { a!: string } ` |
| `externalModules/typeOnly/importsNotUsedAsValues_error.ts` | not supported | two files with one name: `j` |
| `externalModules/typeOnly/importSpecifiers_js.ts` | not supported | JavaScript |
| `externalModules/typeOnly/namespaceImportTypeQuery.ts` | not supported | `any` is not supported, on ` A: undefined as any, ` |
| `externalModules/typeOnly/namespaceImportTypeQuery2.ts` | not supported | `any` is not supported, on ` A: undefined as any, // ok ` |
| `externalModules/typeOnly/namespaceImportTypeQuery3.ts` | not supported | `any` is not supported, on ` A: undefined as any, // ok ` |
| `externalModules/typeOnly/namespaceImportTypeQuery4.ts` | not supported | `any` is not supported, on ` A: undefined as any, // error ` |
| `externalModules/typeOnly/namespaceMemberAccess.ts` | not supported | expected `:` and a type for the class field, on ` class A { a!: string } ` |
| `externalModules/typeOnly/nestedNamespace.ts` | not supported | expected a declaration after `export`, on ` export namespace types { ` |
| `externalModules/typeOnly/preserveValueImports_errors.ts` | not supported | `default` is a reserved keyword and can't be used as a name, on ` export type { A as default }; ` |
| `externalModules/typeOnly/preserveValueImports_importsNotUsedAsValues.ts` | not supported | `any` is not supported, on ` export type C = any; ` |
| `externalModules/typeOnly/preserveValueImports_mixedImports.ts` | checks too little | 0 after the port |
| `externalModules/typeOnly/preserveValueImports_module.ts` | not supported | empty export specifier list, on ` export {}; ` |
| `externalModules/typeOnly/preserveValueImports.ts` | not supported | `export default` is not supported, on ` export default {}; ` |
| `externalModules/typeOnly/renamed.ts` | not supported | expected `:` and a type for the class field, on ` class A { a!: string } ` |
| `externalModules/typeOnly/typeOnlyESMImportFromCJS.ts` | not supported | `.mts` and `.cts` files |
| `externalModules/typeOnly/typeQuery.ts` | not supported | `A` is a class, not a value, so `typeof` has no type to take, on ` let AConstructor: typeof A = null as unknown as (typeof A); ` |
| `externalModules/typeOnlyMerge2.ts` | not supported | `any` is not supported, on ` type A = any; ` |
| `externalModules/typeOnlyMerge3.ts` | not supported | expected `;` after expression, on ` namespace A { ` |
| `externalModules/typesOnlyExternalModuleStillHasInstance.ts` | not supported | expected a declaration after `export`, on ` export namespace M2 { ` |
| `externalModules/umd-augmentation-1.ts` | not supported | `node_modules` packages |
| `externalModules/umd-augmentation-2.ts` | not supported | `node_modules` packages |
| `externalModules/umd-augmentation-3.ts` | not supported | `node_modules` packages |
| `externalModules/umd-augmentation-4.ts` | not supported | `node_modules` packages |
| `externalModules/umd-errors.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/umd1.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/umd2.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/umd3.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/umd4.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/umd5.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/umd6.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/umd7.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/umd8.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/umd9.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/valuesMergingAcrossModules.ts` | not supported | expected `;` after expression, on ` namespace A { ` |
| `externalModules/verbatimModuleSyntaxAmbientConstEnum.ts` | not supported | `node_modules` packages |
| `externalModules/verbatimModuleSyntaxCompat.ts` | not supported | empty export specifier list, on ` export {}; ` |
| `externalModules/verbatimModuleSyntaxCompat2.ts` | not supported | JSON modules |
| `externalModules/verbatimModuleSyntaxCompat3.ts` | not supported | JSON modules |
| `externalModules/verbatimModuleSyntaxCompat4.ts` | not supported | JSON modules |
| `externalModules/verbatimModuleSyntaxConstEnum.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` export const enum E { ` |
| `externalModules/verbatimModuleSyntaxConstEnumUsage.ts` | not supported | expected a number or string literal for enum initializer, on ` a = Foo.a, ` |
| `externalModules/verbatimModuleSyntaxDeclarationFile.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/verbatimModuleSyntaxInternalImportEquals.ts` | not supported | empty export specifier list, on ` export {}; ` |
| `externalModules/verbatimModuleSyntaxNoElisionCJS.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/verbatimModuleSyntaxNoElisionESM.ts` | not supported | `export default` is not supported, on ` export default 1; // ok ` |
| `externalModules/verbatimModuleSyntaxRestrictionsCJS.ts` | not supported | declaration files (`.d.ts`) |
| `externalModules/verbatimModuleSyntaxRestrictionsESM.ts` | not supported | declaration files (`.d.ts`) |
| `fixSignatureCaching.ts` | not supported | expected `;` after declaration, on ` let hasOwnProp = Object.prototype.hasOwnProperty, ` |
| `functions/functionImplementations.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `functions/functionNameConflicts.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `functions/functionOverloadCompatibilityWithVoid01.ts` | not supported | expected `{`, on ` function f(x: string): number; ` |
| `functions/functionOverloadCompatibilityWithVoid02.ts` | not supported | expected `{`, on ` function f(x: string): void; ` |
| `functions/functionOverloadCompatibilityWithVoid03.ts` | not supported | expected `{`, on ` function f(x: string): void; ` |
| `functions/functionOverloadErrors.ts` | not supported | expected `:` and return type, on ` function fn1(x: number = 3); ` |
| `functions/functionOverloadErrorsSyntax.ts` | not supported | required parameter `y` cannot follow an optional parameter, on ` function fn4a(x?: number, y: string); ` |
| `functions/functionParameterObjectRestAndInitializers.ts` | not supported | parameter requires a type annotation, on ` function f({a, ...x}, b = a): any { ` |
| `functions/parameterInitializersBackwardReferencing.ts` | not supported | `any` is not supported, on ` function test0({ a = 0, b = a } = {}): { a: any; b: any; } { ` |
| `functions/parameterInitializersForwardReferencing.2.ts` | not supported | `any` is not supported, on ` function a(): any {} ` |
| `functions/parameterInitializersForwardReferencing1_es6.ts` | duplicate | of `functions/parameterInitializersForwardReferencing1.ts` |
| `functions/strictBindCallApply1.ts` | not supported | `this` is a reserved keyword and can't be used as a name, on ` foo(this: this, a: number, b: string): string { return "" } ` |
| `functions/strictBindCallApply2.ts` | not supported | `this` is a reserved keyword and can't be used as a name, on ` function fn(this: Foo): void {} ` |
| `generators/` | not supported | generators |
| `importAssertion/importAssertion1.ts` | not supported | side-effect-only imports are not supported, on ` import './0' with { type: "json" } ` |
| `importAssertion/importAssertion2.ts` | not supported | empty export specifier list, on ` export {} from './0' with { type: "json" } ` |
| `importAssertion/importAssertion3.ts` | not supported | empty export specifier list, on ` export type {} from './0' with { type: "json" } ` |
| `importAssertion/importAssertion4.ts` | not supported | expected `;` after import statement, on ` import * as f from "./first" with ` |
| `importAssertion/importAssertion5.ts` | not supported | expected `;` after import statement, on ` import * as f from "./first" with { ` |
| `importAttributes/importAttributes1.ts` | not supported | side-effect-only imports are not supported, on ` import './0' with { type: "json" } ` |
| `importAttributes/importAttributes10.ts` | not supported | JSON modules |
| `importAttributes/importAttributes11.ts` | not supported | JSON modules |
| `importAttributes/importAttributes2.ts` | not supported | empty export specifier list, on ` export {} from './0' with { type: "json" } ` |
| `importAttributes/importAttributes3.ts` | not supported | empty export specifier list, on ` export type {} from './0' with { type: "json" } ` |
| `importAttributes/importAttributes4.ts` | not supported | expected `;` after import statement, on ` import * as f from "./first" with ` |
| `importAttributes/importAttributes5.ts` | not supported | expected `;` after import statement, on ` import * as f from "./first" with { ` |
| `importAttributes/importAttributes6.ts` | not supported | `.mts` and `.cts` files |
| `importAttributes/importAttributes7.ts` | not supported | `export default` is not supported, on ` export default { ` |
| `importAttributes/importAttributes8.ts` | not supported | `export default` is not supported, on ` export default { ` |
| `importAttributes/importAttributes9.ts` | not supported | `export default` is not supported, on ` export default {}; ` |
| `importDefer/dynamicImportDefer.ts` | not supported | expected `{` or namespace name after `import`, on ` import.defer("./a.js").then(ns => { ` |
| `importDefer/dynamicImportDeferInvalidStandalone.ts` | not supported | expected `{` or namespace name after `import`, on ` import.defer; ` |
| `importDefer/exportDeferInvalid.ts` | not supported | expected a declaration after `export`, on ` export defer * as ns from "a"; ` |
| `importDefer/importBindingDefer.ts` | not supported | `export default` is not supported, on ` export default 2; ` |
| `importDefer/importBindingDefer2.ts` | not supported | `export default` is not supported, on ` export default 2; ` |
| `importDefer/importDefaultBindingDefer.ts` | not supported | `export default` is not supported, on ` export default function defer(): void { ` |
| `importDefer/importDeferComments.ts` | not supported | empty export specifier list, on ` export {}; ` |
| `importDefer/importDeferDeclaration.ts` | not supported | expected `from` after import specifier list, on ` import defer * as ns from "./a"; ` |
| `importDefer/importDeferFromInvalid.ts` | not supported | `export default` is not supported, on ` export default 2; ` |
| `importDefer/importDeferInvalidDefault.ts` | not supported | `export default` is not supported, on ` export default function foo(): void { ` |
| `importDefer/importDeferInvalidNamed.ts` | not supported | expected `from` after import specifier list, on ` import defer { foo } from "./a"; ` |
| `importDefer/importDeferNamespace.ts` | not supported | expected `from` after import specifier list, on ` import defer * as aNs from "./a"; ` |
| `importDefer/importDeferTypeConflict1.ts` | not supported | expected `from` after import specifier list, on ` import type defer * as ns1 from "./a"; ` |
| `importDefer/importDeferTypeConflict2.ts` | not supported | expected `from` after import specifier list, on ` import defer type * as ns1 from "./a"; ` |
| `importDefer/importEqualsBindingDefer.ts` | not supported | expected a declaration after `export`, on ` export = 2; ` |
| `importDefer/importMetaPropertyInvalidInCall.ts` | not supported | expected `{` or namespace name after `import`, on ` import.foo(); ` |
| `importDefer/typeofImportDefer.ts` | not supported | `import` is a reserved keyword and can't be used as a name, on ` export type X = typeof import.defer("./a").Foo; ` |
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
| `interfaces/interfaceDeclarations/derivedInterfaceDoesNotHideBaseSignatures.ts` | not supported | construct signatures |
| `interfaces/interfaceDeclarations/derivedInterfaceIncompatibleWithBaseIndexer.ts` | not supported | index signatures |
| `interfaces/interfaceDeclarations/interfaceExtendingOptionalChain.ts` | not supported | expected `;` after expression, on ` namespace Foo { ` |
| `interfaces/interfaceDeclarations/interfaceExtendsObjectIntersection.ts` | not supported | intersection types |
| `interfaces/interfaceDeclarations/interfaceExtendsObjectIntersectionErrors.ts` | not supported | intersection types |
| `interfaces/interfaceDeclarations/interfaceThatHidesBaseProperty.ts` | checks too little | 0 after the port |
| `interfaces/interfaceDeclarations/interfaceThatIndirectlyInheritsFromItself.ts` | not supported | expected `;` after expression, on ` namespace Generic { ` |
| `interfaces/interfaceDeclarations/interfaceThatInheritsFromItself.ts` | not supported | expected `{` after interface name, on ` interface Bar implements Bar { // error ` |
| `interfaces/interfaceDeclarations/interfaceWithAccessibilityModifiers.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` public a: any; ` |
| `interfaces/interfaceDeclarations/interfaceWithCallAndConstructSignature.ts` | not supported | construct signatures |
| `interfaces/interfaceDeclarations/interfaceWithCallSignaturesThatHidesBaseSignature.ts` | not supported | `as` to `Derived` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let d: Derived = null as unknown as (Derived); ` |
| `interfaces/interfaceDeclarations/interfaceWithCallSignaturesThatHidesBaseSignature2.ts` | not supported | `as` to `Derived` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let d: Derived = null as unknown as (Derived); ` |
| `interfaces/interfaceDeclarations/interfaceWithConstructSignaturesThatHidesBaseSignature.ts` | not supported | construct signatures |
| `interfaces/interfaceDeclarations/interfaceWithConstructSignaturesThatHidesBaseSignature2.ts` | not supported | construct signatures |
| `interfaces/interfaceDeclarations/interfaceWithMultipleBaseTypes.ts` | not supported | expected `;` after expression, on ` namespace Generic { ` |
| `interfaces/interfaceDeclarations/interfaceWithOverloadedCallAndConstructSignatures.ts` | not supported | construct signatures |
| `interfaces/interfaceDeclarations/interfaceWithPropertyOfEveryType.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `interfaces/interfaceDeclarations/interfaceWithPropertyThatIsPrivateInBaseType2.ts` | not supported | `any` is not supported, on ` x(): any; ` |
| `interfaces/interfaceDeclarations/interfaceWithSpecializedCallAndConstructSignatures.ts` | not supported | construct signatures |
| `interfaces/interfaceDeclarations/interfaceWithStringIndexerHidingBaseTypeIndexer2.ts` | not supported | index signatures |
| `interfaces/interfaceDeclarations/interfaceWithStringIndexerHidingBaseTypeIndexer3.ts` | not supported | index signatures |
| `interfaces/interfacesExtendingClasses/implementingAnInterfaceExtendingClassWithPrivates2.ts` | not supported | expected `:` and a type for the class field, on ` private x!: string; ` |
| `interfaces/interfacesExtendingClasses/implementingAnInterfaceExtendingClassWithProtecteds.ts` | not supported | `protected` is not supported, on ` protected x: string; ` |
| `interfaces/interfacesExtendingClasses/interfaceExtendingClass.ts` | not supported | expected class member name, on ` [x: string]: Object; ` |
| `interfaces/interfacesExtendingClasses/interfaceExtendingClass2.ts` | not supported | expected class member name, on ` [x: string]: Object; ` |
| `interfaces/interfacesExtendingClasses/interfaceExtendingClassWithPrivates.ts` | not supported | expected `:` and a type for the class field, on ` private x!: string; ` |
| `interfaces/interfacesExtendingClasses/interfaceExtendingClassWithPrivates2.ts` | not supported | expected `:` and a type for the class field, on ` private x!: string; ` |
| `interfaces/interfacesExtendingClasses/interfaceExtendingClassWithProtecteds.ts` | not supported | `protected` is not supported, on ` protected x!: string; ` |
| `interfaces/interfacesExtendingClasses/interfaceExtendingClassWithProtecteds2.ts` | not supported | `protected` is not supported, on ` protected x!: string; ` |
| `internalModules/` | not supported | user-declared namespaces |
| `jsdoc/assertionsAndNonReturningFunctions.ts` | not supported | JavaScript |
| `jsdoc/callbackCrossModule.ts` | not supported | JavaScript |
| `jsdoc/callbackOnConstructor.ts` | not supported | JavaScript |
| `jsdoc/callbackTag1.ts` | not supported | JavaScript |
| `jsdoc/callbackTag2.ts` | not supported | JavaScript |
| `jsdoc/callbackTag3.ts` | not supported | JavaScript |
| `jsdoc/callbackTag4.ts` | not supported | JavaScript |
| `jsdoc/callbackTagNamespace.ts` | not supported | JavaScript |
| `jsdoc/callbackTagNestedParameter.ts` | not supported | JavaScript |
| `jsdoc/callbackTagVariadicType.ts` | not supported | JavaScript |
| `jsdoc/callOfPropertylessConstructorFunction.ts` | not supported | JavaScript |
| `jsdoc/checkExportsObjectAssignProperty.ts` | not supported | JavaScript |
| `jsdoc/checkExportsObjectAssignPrototypeProperty.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocOnEndOfFile.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocOptionalParamOrder.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocParamOnVariableDeclaredFunctionExpression.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocParamTag1.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocReturnTag1.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocReturnTag2.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocSatisfiesTag1.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocSatisfiesTag10.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocSatisfiesTag11.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocSatisfiesTag12.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocSatisfiesTag13.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocSatisfiesTag14.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocSatisfiesTag15.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocSatisfiesTag2.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocSatisfiesTag3.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocSatisfiesTag4.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocSatisfiesTag5.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocSatisfiesTag6.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocSatisfiesTag7.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocSatisfiesTag8.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocSatisfiesTag9.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocTypedefInParamTag1.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocTypedefOnlySourceFile.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocTypeTag1.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocTypeTag2.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocTypeTag3.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocTypeTag4.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocTypeTag5.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocTypeTag6.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocTypeTag7.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocTypeTag8.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocTypeTagOnObjectProperty1.ts` | not supported | JavaScript |
| `jsdoc/checkJsdocTypeTagOnObjectProperty2.ts` | not supported | JavaScript |
| `jsdoc/checkObjectDefineProperty.ts` | not supported | JavaScript |
| `jsdoc/checkOtherObjectAssignProperty.ts` | not supported | JavaScript |
| `jsdoc/constructorTagOnClassConstructor.ts` | not supported | JavaScript |
| `jsdoc/constructorTagOnNestedBinaryExpression.ts` | not supported | JavaScript |
| `jsdoc/constructorTagOnObjectLiteralMethod.ts` | not supported | JavaScript |
| `jsdoc/constructorTagWithThisTag.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsClassAccessor.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsClasses.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsClassesErr.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsClassExtendsVisibility.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsClassImplementsGenericsSerialization.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsClassLeadingOptional.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsClassLikeHeuristic.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsClassMethod.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsClassStatic.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsClassStatic2.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsClassStaticMethodAugmentation.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsCommonjsRelativePath.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsComputedNames.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsConstsAsNamespacesWithReferences.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsCrossfileMerge.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsDefault.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsDefaultsErr.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsDocCommentsOnConsts.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsEnums.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsEnumTag.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportAssignedClassExpression.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportAssignedClassExpressionAnonymous.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportAssignedClassExpressionAnonymousWithSub.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportAssignedClassExpressionShadowing.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportAssignedClassInstance1.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportAssignedClassInstance2.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportAssignedClassInstance3.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportAssignedConstructorFunction.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportAssignedConstructorFunctionWithSub.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportAssignedVisibility.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportAssignmentExpressionPlusSecondary.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportAssignmentWithKeywordName.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportDefinePropertyEmit.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportDoubleAssignmentInClosure.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportedClassAliases.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportForms.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportFormsErr.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportSpecifierNonlocal.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsExportSubAssignments.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsFunctionClassesCjsExportAssignment.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsFunctionJSDoc.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsFunctionKeywordProp.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsFunctionKeywordPropExhaustive.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsFunctionLikeClasses.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsFunctionLikeClasses2.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsFunctionPrototypeStatic.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsFunctions.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsFunctionsCjs.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsFunctionWithDefaultAssignedMember.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsGetterSetter.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsImportAliasExposedWithinNamespace.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsImportAliasExposedWithinNamespaceCjs.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsImportNamespacedType.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsImportTypeBundled.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsInterfaces.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsJSDocRedirectedLookups.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsJson.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsMissingGenerics.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsMissingTypeParameters.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsModuleReferenceHasEmit.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsMultipleExportFromMerge.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsNestedParams.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsNonIdentifierInferredNames.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsOptionalTypeLiteralProps1.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsOptionalTypeLiteralProps2.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsPackageJson.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsParameterTagReusesInputNodeInEmit1.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsParameterTagReusesInputNodeInEmit2.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsPrivateFields01.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsReactComponents.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsReexportAliases.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsReexportAliasesEsModuleInterop.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsReexportedCjsAlias.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsReferenceToClassInstanceCrossFile.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsRestArgsWithThisTypeInJSDocFunction.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsReusesExistingNodesMappingJSDocTypes.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsReusesExistingTypeAnnotations.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsSubclassWithExplicitNoArgumentConstructor.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsThisTypes.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsTypeAliases.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsTypedefAndImportTypes.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsTypedefAndLatebound.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsTypedefDescriptionsPreserved.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsTypedefFunction.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsTypedefPropertyAndExportAssignment.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsTypeReassignmentFromDeclaration.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsTypeReassignmentFromDeclaration2.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsTypeReferences.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsTypeReferences2.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsTypeReferences3.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsTypeReferences4.ts` | not supported | JavaScript |
| `jsdoc/declarations/jsDeclarationsUniqueSymbolUsage.ts` | not supported | JavaScript |
| `jsdoc/enumTag.ts` | not supported | JavaScript |
| `jsdoc/enumTagCircularReference.ts` | not supported | JavaScript |
| `jsdoc/enumTagImported.ts` | not supported | JavaScript |
| `jsdoc/enumTagOnExports.ts` | not supported | JavaScript |
| `jsdoc/enumTagOnExports2.ts` | not supported | JavaScript |
| `jsdoc/enumTagUseBeforeDefCrash.ts` | not supported | JavaScript |
| `jsdoc/errorIsolation.ts` | not supported | JavaScript |
| `jsdoc/errorOnFunctionReturnType.ts` | not supported | JavaScript |
| `jsdoc/exportedAliasedEnumTag.ts` | not supported | JavaScript |
| `jsdoc/exportedEnumTypeAndValue.ts` | not supported | JavaScript |
| `jsdoc/extendsTag1.ts` | not supported | JavaScript |
| `jsdoc/extendsTag2.ts` | not supported | JavaScript |
| `jsdoc/extendsTag3.ts` | not supported | JavaScript |
| `jsdoc/extendsTag4.ts` | not supported | JavaScript |
| `jsdoc/extendsTag5.ts` | not supported | JavaScript |
| `jsdoc/extendsTag6.ts` | not supported | JavaScript |
| `jsdoc/extendsTagEmit.ts` | not supported | JavaScript |
| `jsdoc/importDeferJsdoc.ts` | not supported | JavaScript |
| `jsdoc/importTag1.ts` | not supported | JavaScript |
| `jsdoc/importTag10.ts` | not supported | JavaScript |
| `jsdoc/importTag11.ts` | not supported | JavaScript |
| `jsdoc/importTag12.ts` | not supported | JavaScript |
| `jsdoc/importTag13.ts` | not supported | JavaScript |
| `jsdoc/importTag14.ts` | not supported | JavaScript |
| `jsdoc/importTag15.ts` | not supported | JavaScript |
| `jsdoc/importTag16.ts` | not supported | JavaScript |
| `jsdoc/importTag17.ts` | not supported | JavaScript |
| `jsdoc/importTag18.ts` | not supported | JavaScript |
| `jsdoc/importTag19.ts` | not supported | JavaScript |
| `jsdoc/importTag2.ts` | not supported | JavaScript |
| `jsdoc/importTag20.ts` | not supported | JavaScript |
| `jsdoc/importTag21.ts` | not supported | JavaScript |
| `jsdoc/importTag22.ts` | not supported | `node_modules` packages |
| `jsdoc/importTag23.ts` | not supported | JavaScript |
| `jsdoc/importTag24.ts` | not supported | JavaScript |
| `jsdoc/importTag25.ts` | not supported | JavaScript |
| `jsdoc/importTag3.ts` | not supported | JavaScript |
| `jsdoc/importTag4.ts` | not supported | JavaScript |
| `jsdoc/importTag5.ts` | not supported | JavaScript |
| `jsdoc/importTag6.ts` | not supported | JavaScript |
| `jsdoc/importTag7.ts` | not supported | JavaScript |
| `jsdoc/importTag8.ts` | not supported | JavaScript |
| `jsdoc/importTag9.ts` | not supported | JavaScript |
| `jsdoc/inferThis.ts` | not supported | JavaScript |
| `jsdoc/instantiateTemplateTagTypeParameterOnVariableStatement.ts` | not supported | JavaScript |
| `jsdoc/jsdocAccessibilityTags.ts` | not supported | JavaScript |
| `jsdoc/jsdocAccessibilityTagsDeclarations.ts` | not supported | JavaScript |
| `jsdoc/jsdocAugments_errorInExtendsExpression.ts` | not supported | JavaScript |
| `jsdoc/jsdocAugments_nameMismatch.ts` | not supported | JavaScript |
| `jsdoc/jsdocAugments_noExtends.ts` | not supported | JavaScript |
| `jsdoc/jsdocAugments_notAClass.ts` | not supported | JavaScript |
| `jsdoc/jsdocAugments_qualifiedName.ts` | not supported | JavaScript |
| `jsdoc/jsdocAugments_withTypeParameter.ts` | not supported | JavaScript |
| `jsdoc/jsdocAugmentsMissingType.ts` | not supported | JavaScript |
| `jsdoc/jsdocBindingInUnreachableCode.ts` | not supported | JavaScript |
| `jsdoc/jsdocCatchClauseWithTypeAnnotation.ts` | not supported | JavaScript |
| `jsdoc/jsdocDisallowedInTypescript.ts` | not supported | expected identifier after `.` in type name, on ` let ara: Array.<number> = [1,2,3]; ` |
| `jsdoc/jsdocFunction_missingReturn.ts` | not supported | JavaScript |
| `jsdoc/jsdocFunctionType.ts` | not supported | JavaScript |
| `jsdoc/jsdocImplements_class.ts` | not supported | JavaScript |
| `jsdoc/jsdocImplements_interface_multiple.ts` | not supported | JavaScript |
| `jsdoc/jsdocImplements_interface.ts` | not supported | JavaScript |
| `jsdoc/jsdocImplements_missingType.ts` | not supported | JavaScript |
| `jsdoc/jsdocImplements_namespacedInterface.ts` | not supported | JavaScript |
| `jsdoc/jsdocImplements_properties.ts` | not supported | JavaScript |
| `jsdoc/jsdocImplements_signatures.ts` | not supported | JavaScript |
| `jsdoc/jsdocImplementsTag.ts` | not supported | JavaScript |
| `jsdoc/jsdocImportType.ts` | not supported | JavaScript |
| `jsdoc/jsdocImportType2.ts` | not supported | JavaScript |
| `jsdoc/jsdocImportTypeReferenceToClassAlias.ts` | not supported | JavaScript |
| `jsdoc/jsdocImportTypeReferenceToCommonjsModule.ts` | not supported | JavaScript |
| `jsdoc/jsdocImportTypeReferenceToESModule.ts` | not supported | JavaScript |
| `jsdoc/jsdocImportTypeReferenceToStringLiteral.ts` | not supported | JavaScript |
| `jsdoc/jsdocIndexSignature.ts` | not supported | index signatures |
| `jsdoc/jsdocLinkTag1.ts` | checks too little | 0 after the port |
| `jsdoc/jsdocLinkTag2.ts` | not supported | JavaScript |
| `jsdoc/jsdocLinkTag5.ts` | checks too little | 0 after the port |
| `jsdoc/jsdocLinkTag6.ts` | checks too little | 0 after the port |
| `jsdoc/jsdocLinkTag7.ts` | not supported | JavaScript |
| `jsdoc/jsdocLinkTag8.ts` | not supported | JavaScript |
| `jsdoc/jsdocLinkTag9.ts` | checks too little | 0 after the port |
| `jsdoc/jsdocLiteral.ts` | not supported | JavaScript |
| `jsdoc/jsdocNeverUndefinedNull.ts` | not supported | JavaScript |
| `jsdoc/jsdocOuterTypeParameters1.ts` | not supported | JavaScript |
| `jsdoc/jsdocOuterTypeParameters2.ts` | not supported | JavaScript |
| `jsdoc/jsdocOuterTypeParameters3.ts` | not supported | JavaScript |
| `jsdoc/jsdocOverrideTag1.ts` | not supported | JavaScript |
| `jsdoc/jsdocParamTag2.ts` | not supported | JavaScript |
| `jsdoc/jsdocParamTagTypeLiteral.ts` | not supported | JavaScript |
| `jsdoc/jsdocParseBackquotedParamName.ts` | not supported | JavaScript |
| `jsdoc/jsdocParseDotDotDotInJSDocFunction.ts` | not supported | JavaScript |
| `jsdoc/jsdocParseErrorsInTypescript.ts` | not supported | expected expression, on ` function parse1(n: number=): void { } ` |
| `jsdoc/jsdocParseHigherOrderFunction.ts` | not supported | JavaScript |
| `jsdoc/jsdocParseMatchingBackticks.ts` | not supported | JavaScript |
| `jsdoc/jsdocParseParenthesizedJSDocParameter.ts` | not supported | JavaScript |
| `jsdoc/jsdocParseStarEquals.ts` | not supported | JavaScript |
| `jsdoc/jsdocPostfixEqualsAddsOptionality.ts` | not supported | JavaScript |
| `jsdoc/jsdocPrefixPostfixParsing.ts` | not supported | JavaScript |
| `jsdoc/jsdocPrivateName1.ts` | not supported | JavaScript |
| `jsdoc/jsdocPrivateName2.ts` | not supported | JavaScript |
| `jsdoc/jsdocPrototypePropertyAccessWithType.ts` | not supported | JavaScript |
| `jsdoc/jsdocReadonly.ts` | not supported | JavaScript |
| `jsdoc/jsdocReadonlyDeclarations.ts` | not supported | JavaScript |
| `jsdoc/jsdocReturnTag1.ts` | not supported | JavaScript |
| `jsdoc/jsdocSignatureOnReturnedFunction.ts` | not supported | JavaScript |
| `jsdoc/jsdocTemplateClass.ts` | not supported | JavaScript |
| `jsdoc/jsdocTemplateConstructorFunction.ts` | not supported | JavaScript |
| `jsdoc/jsdocTemplateConstructorFunction2.ts` | not supported | JavaScript |
| `jsdoc/jsdocTemplateTag.ts` | not supported | JavaScript |
| `jsdoc/jsdocTemplateTag2.ts` | not supported | JavaScript |
| `jsdoc/jsdocTemplateTag3.ts` | not supported | JavaScript |
| `jsdoc/jsdocTemplateTag4.ts` | not supported | JavaScript |
| `jsdoc/jsdocTemplateTag5.ts` | not supported | JavaScript |
| `jsdoc/jsdocTemplateTag6.ts` | not supported | JavaScript |
| `jsdoc/jsdocTemplateTag7.ts` | not supported | JavaScript |
| `jsdoc/jsdocTemplateTag8.ts` | not supported | JavaScript |
| `jsdoc/jsdocTemplateTagDefault.ts` | not supported | JavaScript |
| `jsdoc/jsdocTemplateTagNameResolution.ts` | not supported | JavaScript |
| `jsdoc/jsdocThisType.ts` | not supported | JavaScript |
| `jsdoc/jsdocTwoLineTypedef.ts` | checks too little | 0 after the port |
| `jsdoc/jsdocTypeDefAtStartOfFile.ts` | not supported | JavaScript |
| `jsdoc/jsdocTypeFromChainedAssignment.ts` | not supported | JavaScript |
| `jsdoc/jsdocTypeFromChainedAssignment2.ts` | not supported | JavaScript |
| `jsdoc/jsdocTypeFromChainedAssignment3.ts` | not supported | JavaScript |
| `jsdoc/jsdocTypeReferenceExports.ts` | not supported | JavaScript |
| `jsdoc/jsdocTypeReferenceToImport.ts` | not supported | JavaScript |
| `jsdoc/jsdocTypeReferenceToImportOfClassExpression.ts` | not supported | JavaScript |
| `jsdoc/jsdocTypeReferenceToImportOfFunctionExpression.ts` | not supported | JavaScript |
| `jsdoc/jsdocTypeReferenceToMergedClass.ts` | not supported | JavaScript |
| `jsdoc/jsdocTypeReferenceToValue.ts` | not supported | JavaScript |
| `jsdoc/jsdocTypeReferenceUseBeforeDef.ts` | not supported | JavaScript |
| `jsdoc/jsdocTypeTag.ts` | not supported | JavaScript |
| `jsdoc/jsdocTypeTagCast.ts` | not supported | JavaScript |
| `jsdoc/jsdocTypeTagOnParameter1.ts` | not supported | JavaScript |
| `jsdoc/jsdocTypeTagParameterType.ts` | not supported | JavaScript |
| `jsdoc/jsdocTypeTagRequiredParameters.ts` | not supported | JavaScript |
| `jsdoc/jsdocVariableDeclarationWithTypeAnnotation.ts` | not supported | JavaScript |
| `jsdoc/jsdocVariadicType.ts` | not supported | JavaScript |
| `jsdoc/linkTagEmit1.ts` | not supported | JavaScript |
| `jsdoc/moduleExportsElementAccessAssignment.ts` | not supported | JavaScript |
| `jsdoc/moduleExportsElementAccessAssignment2.ts` | not supported | JavaScript |
| `jsdoc/noAssertForUnparseableTypedefs.ts` | not supported | JavaScript |
| `jsdoc/noDuplicateJsdoc1.ts` | not supported | JavaScript |
| `jsdoc/overloadTag1.ts` | not supported | JavaScript |
| `jsdoc/overloadTag2.ts` | not supported | JavaScript |
| `jsdoc/overloadTag3.ts` | not supported | JavaScript |
| `jsdoc/paramTagBracketsAddOptionalUndefined.ts` | not supported | JavaScript |
| `jsdoc/paramTagNestedWithoutTopLevelObject.ts` | not supported | JavaScript |
| `jsdoc/paramTagNestedWithoutTopLevelObject2.ts` | not supported | JavaScript |
| `jsdoc/paramTagNestedWithoutTopLevelObject3.ts` | not supported | JavaScript |
| `jsdoc/paramTagNestedWithoutTopLevelObject4.ts` | not supported | JavaScript |
| `jsdoc/paramTagOnCallExpression.ts` | not supported | JavaScript |
| `jsdoc/paramTagOnFunctionUsingArguments.ts` | not supported | JavaScript |
| `jsdoc/paramTagTypeResolution.ts` | not supported | JavaScript |
| `jsdoc/paramTagTypeResolution2.ts` | not supported | JavaScript |
| `jsdoc/paramTagWrapping.ts` | not supported | JavaScript |
| `jsdoc/parseLinkTag.ts` | not supported | `let` declaration requires an initializer, on ` let x; ` |
| `jsdoc/parseThrowsTag.ts` | checks too little | 0 after the port |
| `jsdoc/returnTagTypeGuard.ts` | not supported | JavaScript |
| `jsdoc/seeTag1.ts` | not supported | expected `;` after expression, on ` namespace NS { ` |
| `jsdoc/seeTag3.ts` | not supported | JavaScript |
| `jsdoc/seeTag4.ts` | not supported | JavaScript |
| `jsdoc/syntaxErrors.ts` | not supported | JavaScript |
| `jsdoc/templateInsideCallback.ts` | not supported | JavaScript |
| `jsdoc/thisPrototypeMethodCompoundAssignment.ts` | not supported | expected expression, on ` Element.prototype.remove ??= function () { ` |
| `jsdoc/thisPrototypeMethodCompoundAssignmentJs.ts` | not supported | JavaScript |
| `jsdoc/thisTag1.ts` | not supported | JavaScript |
| `jsdoc/thisTag2.ts` | not supported | JavaScript |
| `jsdoc/thisTag3.ts` | not supported | JavaScript |
| `jsdoc/tsNoCheckForTypescriptComments2.ts` | duplicate | of `jsdoc/tsNoCheckForTypescript.ts` |
| `jsdoc/typedefCrossModule.ts` | not supported | JavaScript |
| `jsdoc/typedefCrossModule2.ts` | not supported | JavaScript |
| `jsdoc/typedefCrossModule3.ts` | not supported | JavaScript |
| `jsdoc/typedefCrossModule4.ts` | not supported | JavaScript |
| `jsdoc/typedefCrossModule5.ts` | not supported | JavaScript |
| `jsdoc/typedefDuplicateTypeDeclaration.ts` | not supported | JavaScript |
| `jsdoc/typedefInnerNamepaths.ts` | not supported | JavaScript |
| `jsdoc/typedefMultipleTypeParameters.ts` | not supported | JavaScript |
| `jsdoc/typedefOnSemicolonClassElement.ts` | not supported | JavaScript |
| `jsdoc/typedefOnStatements.ts` | not supported | JavaScript |
| `jsdoc/typedefScope1.ts` | not supported | JavaScript |
| `jsdoc/typedefTagExtraneousProperty.ts` | not supported | JavaScript |
| `jsdoc/typedefTagNested.ts` | not supported | JavaScript |
| `jsdoc/typedefTagTypeResolution.ts` | not supported | JavaScript |
| `jsdoc/typedefTagWrapping.ts` | not supported | JavaScript |
| `jsdoc/typeParameterExtendsUnionConstraintDistributed.ts` | not supported | expected `,` or `>`, on ` function f<T extends A>(a: T): A & T { return a; } // Shouldn't error ` |
| `jsdoc/typeTagCircularReferenceOnConstructorFunction.ts` | not supported | JavaScript |
| `jsdoc/typeTagModuleExports.ts` | not supported | JavaScript |
| `jsdoc/typeTagNoErasure.ts` | not supported | JavaScript |
| `jsdoc/typeTagOnPropertyAssignment.ts` | not supported | JavaScript |
| `jsdoc/typeTagPrototypeAssignment.ts` | not supported | JavaScript |
| `jsdoc/typeTagWithGenericSignature.ts` | not supported | JavaScript |
| `jsx/jsxAttributeInitializer.ts` | not supported | JSX |
| `jsx/jsxUnclosedParserRecovery.ts` | not supported | JSX |
| `jsx/tsxEmitSpreadAttribute.ts` | not supported | JSX |
| `jsx/tsxReactEmitSpreadAttribute.ts` | not supported | JSX |
| `moduleResolution/allowImportingTsExtensions.ts` | not supported | JSX |
| `moduleResolution/allowImportingTypesDtsExtension.ts` | not supported | declaration files (`.d.ts`) |
| `moduleResolution/bundler/bundlerCommonJS.ts` | not supported | `node_modules` packages |
| `moduleResolution/bundler/bundlerConditionsExcludesNode.ts` | not supported | JavaScript |
| `moduleResolution/bundler/bundlerDirectoryModule.ts` | not supported | JavaScript |
| `moduleResolution/bundler/bundlerImportESM.ts` | not supported | `.mts` and `.cts` files |
| `moduleResolution/bundler/bundlerImportTsExtensions.ts` | not supported | JavaScript |
| `moduleResolution/bundler/bundlerNodeModules1.ts` | not supported | JavaScript |
| `moduleResolution/bundler/bundlerOptionsCompat.ts` | not supported | JSON modules |
| `moduleResolution/bundler/bundlerRelative1.ts` | not supported | JSON modules |
| `moduleResolution/bundler/bundlerSyntaxRestrictions.ts` | not supported | JavaScript |
| `moduleResolution/conditionalExportsResolutionFallback.ts` | not supported | JavaScript |
| `moduleResolution/conditionalExportsResolutionFallbackNull.ts` | not supported | JavaScript |
| `moduleResolution/customConditions.ts` | not supported | `node_modules` packages |
| `moduleResolution/declarationNotFoundPackageBundlesTypes.ts` | not supported | JavaScript |
| `moduleResolution/extensionLoadingPriority.ts` | not supported | JavaScript |
| `moduleResolution/nestedPackageJsonRedirect.ts` | not supported | `node_modules` packages |
| `moduleResolution/node10AlternateResult_noResolution.ts` | not supported | `node_modules` packages |
| `moduleResolution/node10Alternateresult_noTypes.ts` | not supported | JavaScript |
| `moduleResolution/node10IsNode_node.ts` | not supported | `node_modules` packages |
| `moduleResolution/node10IsNode_node10.ts` | not supported | `node_modules` packages |
| `moduleResolution/nodeModulesAtTypesPriority.ts` | not supported | JavaScript |
| `moduleResolution/packageJsonExportsOptionsCompat.ts` | not supported | JSON modules |
| `moduleResolution/packageJsonImportsExportsOptionCompat.ts` | checks too little | 0 after the port |
| `moduleResolution/packageJsonMain_isNonRecursive.ts` | not supported | JavaScript |
| `moduleResolution/packageJsonMain.ts` | not supported | JavaScript |
| `moduleResolution/resolutionModeCache.ts` | not supported | `node_modules` packages |
| `moduleResolution/resolutionModeImportType1.ts` | not supported | `node_modules` packages |
| `moduleResolution/resolutionModeTripleSlash1.ts` | not supported | `node_modules` packages |
| `moduleResolution/resolutionModeTripleSlash2.ts` | not supported | `node_modules` packages |
| `moduleResolution/resolutionModeTripleSlash3.ts` | not supported | `node_modules` packages |
| `moduleResolution/resolutionModeTripleSlash4.ts` | not supported | `node_modules` packages |
| `moduleResolution/resolutionModeTripleSlash5.ts` | not supported | `node_modules` packages |
| `moduleResolution/resolutionModeTypeOnlyImport1.ts` | not supported | `node_modules` packages |
| `moduleResolution/resolvesWithoutExportsDiagnostic1.ts` | not supported | JavaScript |
| `moduleResolution/scopedPackages.ts` | not supported | `node_modules` packages |
| `moduleResolution/scopedPackagesClassic.ts` | not supported | `node_modules` packages |
| `moduleResolution/selfNameModuleAugmentation.ts` | not supported | JavaScript |
| `moduleResolution/typesVersions.ambientModules.ts` | not supported | `node_modules` packages |
| `moduleResolution/typesVersions.emptyTypes.ts` | not supported | JSON modules |
| `moduleResolution/typesVersions.justIndex.ts` | not supported | JSON modules |
| `moduleResolution/typesVersions.multiFile.ts` | not supported | `node_modules` packages |
| `moduleResolution/untypedModuleImport_allowJs.ts` | not supported | JavaScript |
| `moduleResolution/untypedModuleImport_noImplicitAny_relativePath.ts` | not supported | JavaScript |
| `moduleResolution/untypedModuleImport_noImplicitAny_scoped.ts` | not supported | JavaScript |
| `moduleResolution/untypedModuleImport_noImplicitAny_typesForPackageExist.ts` | not supported | JavaScript |
| `moduleResolution/untypedModuleImport_noImplicitAny.ts` | not supported | JavaScript |
| `moduleResolution/untypedModuleImport_vsAmbient.ts` | not supported | JavaScript |
| `moduleResolution/untypedModuleImport_withAugmentation.ts` | not supported | JavaScript |
| `moduleResolution/untypedModuleImport.ts` | not supported | JavaScript |
| `node/allowJs/nodeAllowJsPackageSelfName.ts` | not supported | JavaScript |
| `node/allowJs/nodeAllowJsPackageSelfName2.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJs1.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsCjsFromJs.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsConditionalPackageExports.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsDynamicImport.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsExportAssignment.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsExportlessJsModuleDetectionAuto.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsGeneratedNameCollisions.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsImportAssignment.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsImportHelpersCollisions1.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsImportHelpersCollisions2.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsImportHelpersCollisions3.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsImportMeta.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsPackageExports.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsPackageImports.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsPackagePatternExports.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsPackagePatternExportsExclude.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsPackagePatternExportsTrailers.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsSynchronousCallErrors.ts` | not supported | JavaScript |
| `node/allowJs/nodeModulesAllowJsTopLevelAwait.ts` | not supported | JavaScript |
| `node/esmModuleExports1.ts` | not supported | JavaScript |
| `node/esmModuleExports2.ts` | not supported | JavaScript |
| `node/esmModuleExports3.ts` | not supported | JavaScript |
| `node/legacyNodeModulesExportsSpecifierGenerationConditions.ts` | not supported | `node_modules` packages |
| `node/nodeModules1.ts` | not supported | `.mts` and `.cts` files |
| `node/nodeModulesCJSEmit1.ts` | not supported | JavaScript |
| `node/nodeModulesCjsFormatFileAlwaysHasDefault.ts` | not supported | JSON modules |
| `node/nodeModulesCJSResolvingToESM1_emptyPackageJson.ts` | not supported | JavaScript |
| `node/nodeModulesCJSResolvingToESM2_cjsPackageJson.ts` | not supported | JavaScript |
| `node/nodeModulesCJSResolvingToESM3_modulePackageJson.ts` | not supported | JavaScript |
| `node/nodeModulesCJSResolvingToESM4_noPackageJson.ts` | not supported | JavaScript |
| `node/nodeModulesConditionalPackageExports.ts` | not supported | `.mts` and `.cts` files |
| `node/nodeModulesDeclarationEmitDynamicImportWithPackageExports.ts` | not supported | `.mts` and `.cts` files |
| `node/nodeModulesDeclarationEmitWithPackageExports.ts` | not supported | `.mts` and `.cts` files |
| `node/nodeModulesDynamicImport.ts` | not supported | JSON modules |
| `node/nodeModulesExportAssignments.ts` | not supported | JSON modules |
| `node/nodeModulesExportsBlocksSpecifierResolution.ts` | not supported | `node_modules` packages |
| `node/nodeModulesExportsBlocksTypesVersions.ts` | not supported | JavaScript |
| `node/nodeModulesExportsDoubleAsterisk.ts` | not supported | `node_modules` packages |
| `node/nodeModulesExportsSourceTs.ts` | not supported | `node_modules` packages |
| `node/nodeModulesExportsSpecifierGenerationConditions.ts` | not supported | `node_modules` packages |
| `node/nodeModulesExportsSpecifierGenerationDirectory.ts` | not supported | `node_modules` packages |
| `node/nodeModulesExportsSpecifierGenerationPattern.ts` | not supported | `node_modules` packages |
| `node/nodeModulesForbidenSyntax.ts` | not supported | `.mts` and `.cts` files |
| `node/nodeModulesGeneratedNameCollisions.ts` | not supported | JSON modules |
| `node/nodeModulesImportAssertions.ts` | not supported | `.mts` and `.cts` files |
| `node/nodeModulesImportAssignments.ts` | not supported | JSON modules |
| `node/nodeModulesImportAttributes.ts` | not supported | `.mts` and `.cts` files |
| `node/nodeModulesImportAttributesModeDeclarationEmit1.ts` | not supported | `node_modules` packages |
| `node/nodeModulesImportAttributesModeDeclarationEmit2.ts` | not supported | `node_modules` packages |
| `node/nodeModulesImportAttributesModeDeclarationEmitErrors.ts` | not supported | `node_modules` packages |
| `node/nodeModulesImportAttributesTypeModeDeclarationEmit.ts` | not supported | `node_modules` packages |
| `node/nodeModulesImportAttributesTypeModeDeclarationEmitErrors.ts` | not supported | `node_modules` packages |
| `node/nodeModulesImportHelpersCollisions.ts` | not supported | JSON modules |
| `node/nodeModulesImportHelpersCollisions2.ts` | not supported | JSON modules |
| `node/nodeModulesImportHelpersCollisions3.ts` | not supported | JSON modules |
| `node/nodeModulesImportMeta.ts` | not supported | JSON modules |
| `node/nodeModulesImportModeDeclarationEmit1.ts` | not supported | `node_modules` packages |
| `node/nodeModulesImportModeDeclarationEmit2.ts` | not supported | `node_modules` packages |
| `node/nodeModulesImportModeDeclarationEmitErrors1.ts` | not supported | `node_modules` packages |
| `node/nodeModulesImportResolutionIntoExport.ts` | not supported | `.mts` and `.cts` files |
| `node/nodeModulesImportResolutionNoCycle.ts` | not supported | `.mts` and `.cts` files |
| `node/nodeModulesImportTypeModeDeclarationEmit1.ts` | not supported | `node_modules` packages |
| `node/nodeModulesImportTypeModeDeclarationEmitErrors1.ts` | not supported | `node_modules` packages |
| `node/nodeModulesJson.ts` | not supported | `node_modules` packages |
| `node/nodeModulesNoDirectoryModule.ts` | not supported | `node_modules` packages |
| `node/nodeModulesPackageExports.ts` | not supported | `.mts` and `.cts` files |
| `node/nodeModulesPackageImports.ts` | not supported | `.mts` and `.cts` files |
| `node/nodeModulesPackageImportsRootWildcard.ts` | not supported | JSON modules |
| `node/nodeModulesPackageImportsRootWildcardNode16.ts` | not supported | JSON modules |
| `node/nodeModulesPackagePatternExports.ts` | not supported | `.mts` and `.cts` files |
| `node/nodeModulesPackagePatternExportsExclude.ts` | not supported | `.mts` and `.cts` files |
| `node/nodeModulesPackagePatternExportsTrailers.ts` | not supported | `.mts` and `.cts` files |
| `node/nodeModulesResolveJsonModule.ts` | not supported | `.mts` and `.cts` files |
| `node/nodeModulesSynchronousCallErrors.ts` | not supported | JSON modules |
| `node/nodeModulesTopLevelAwait.ts` | not supported | JSON modules |
| `node/nodeModulesTripleSlashReferenceModeDeclarationEmit1.ts` | not supported | `node_modules` packages |
| `node/nodeModulesTripleSlashReferenceModeDeclarationEmit2.ts` | not supported | `node_modules` packages |
| `node/nodeModulesTripleSlashReferenceModeDeclarationEmit3.ts` | not supported | `node_modules` packages |
| `node/nodeModulesTripleSlashReferenceModeDeclarationEmit4.ts` | not supported | `node_modules` packages |
| `node/nodeModulesTripleSlashReferenceModeDeclarationEmit5.ts` | not supported | `node_modules` packages |
| `node/nodeModulesTripleSlashReferenceModeDeclarationEmit6.ts` | not supported | `node_modules` packages |
| `node/nodeModulesTripleSlashReferenceModeDeclarationEmit7.ts` | not supported | `node_modules` packages |
| `node/nodeModulesTripleSlashReferenceModeOverride1.ts` | not supported | `node_modules` packages |
| `node/nodeModulesTripleSlashReferenceModeOverride2.ts` | not supported | `node_modules` packages |
| `node/nodeModulesTripleSlashReferenceModeOverride3.ts` | not supported | `node_modules` packages |
| `node/nodeModulesTripleSlashReferenceModeOverride4.ts` | not supported | `node_modules` packages |
| `node/nodeModulesTripleSlashReferenceModeOverride5.ts` | not supported | `node_modules` packages |
| `node/nodeModulesTripleSlashReferenceModeOverrideModeError.ts` | not supported | `node_modules` packages |
| `node/nodeModulesTripleSlashReferenceModeOverrideOldResolutionError.ts` | not supported | `node_modules` packages |
| `node/nodeModulesTypesVersionPackageExports.ts` | not supported | `.mts` and `.cts` files |
| `node/nodePackageSelfName.ts` | not supported | `.mts` and `.cts` files |
| `node/nodePackageSelfNameScoped.ts` | not supported | `.mts` and `.cts` files |
| `nonjsExtensions/declarationFileForHtmlFileWithinDeclarationFile.ts` | not supported | declaration files (`.d.ts`) |
| `nonjsExtensions/declarationFileForHtmlImport.ts` | not supported | declaration files (`.d.ts`) |
| `nonjsExtensions/declarationFileForJsonImport.ts` | not supported | JSON modules |
| `nonjsExtensions/declarationFileForTsJsImport.ts` | not supported | JSON modules |
| `nonjsExtensions/declarationFilesForNodeNativeModules.ts` | not supported | JSON modules |
| `override/override_js1.ts` | not supported | JavaScript |
| `override/override_js2.ts` | not supported | JavaScript |
| `override/override_js3.ts` | not supported | JavaScript |
| `override/override_js4.ts` | not supported | JavaScript |
| `override/override1.ts` | not supported | expected `:` and a type for the class field, on ` override foo (v: string): void {} ` |
| `override/override10.ts` | not supported | expected `;` after expression, on ` abstract class Base { ` |
| `override/override11.ts` | not supported | parameter requires a type annotation, on ` constructor (override public foo: number) { ` |
| `override/override12.ts` | not supported | expected `:` and a type for the class field, on ` override m1(): number { ` |
| `override/override14.ts` | not supported | expected `:` and a type for the class field, on ` declare property: number ` |
| `override/override15.ts` | not supported | expected `:` and a type for the class field, on ` override doSomethang(): void {} ` |
| `override/override16.ts` | not supported | expected `:` and a type for the class field, on ` override foo: string = "string"; ` |
| `override/override17.ts` | duplicate | of `override/override12.ts` |
| `override/override18.ts` | not supported | expected `:` and a type for the class field, on ` override foo: string = "string"; ` |
| `override/override19.ts` | not supported | construct signatures |
| `override/override2.ts` | not supported | expected `;` after expression, on ` abstract class AB { ` |
| `override/override20.ts` | not supported | construct signatures |
| `override/override21.ts` | not supported | expected `:` and a type for the class field, on ` override [foo](): void { } ` |
| `override/override3.ts` | not supported | expected `;` after expression, on ` declare class B { ` |
| `override/override4.ts` | not supported | expected `:` and a type for the class field, on ` override p2: number = 3; ` |
| `override/override5.ts` | not supported | expected `:` and a type for the class field, on ` declare p1: number ` |
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
| `parser/ecmascript2021/numericSeparators/parser.numericSeparators.binary.ts` | checks too little | 0 after the port |
| `parser/ecmascript2021/numericSeparators/parser.numericSeparators.decimal.ts` | checks too little | 0 after the port |
| `parser/ecmascript2021/numericSeparators/parser.numericSeparators.hex.ts` | checks too little | 0 after the port |
| `parser/ecmascript2021/numericSeparators/parser.numericSeparators.octal.ts` | checks too little | 0 after the port |
| `parser/ecmascript2021/numericSeparators/parser.numericSeparators.unicodeEscape.ts` | not supported | invalid unicode escape: expected `}`, on ` "\u{10_ffff}" ` |
| `parser/ecmascript3/Accessors/parserES3Accessors2.ts` | not supported | parameter requires a type annotation, on ` set Foo(a) { } ` |
| `parser/ecmascript3/Accessors/parserES3Accessors3.ts` | not supported | expected `,` or `}`, on ` let v = { get Foo() { } }; ` |
| `parser/ecmascript3/Accessors/parserES3Accessors4.ts` | not supported | expected `,` or `}`, on ` let v = { set Foo(a) { } }; ` |
| `parser/ecmascript5/Accessors/parserAccessors1.ts` | duplicate | of `parser/ecmascript3/Accessors/parserES3Accessors1.ts` |
| `parser/ecmascript5/Accessors/parserAccessors10.ts` | not supported | expected `,` or `}`, on ` public get foo() { } ` |
| `parser/ecmascript5/Accessors/parserAccessors2.ts` | not supported | parameter requires a type annotation, on ` set Foo(a) { } ` |
| `parser/ecmascript5/Accessors/parserAccessors3.ts` | not supported | expected `,` or `}`, on ` let v = { get Foo() { } }; ` |
| `parser/ecmascript5/Accessors/parserAccessors4.ts` | not supported | expected `,` or `}`, on ` let v = { set Foo(a) { } }; ` |
| `parser/ecmascript5/Accessors/parserAccessors5.ts` | not supported | expected `;` after expression, on ` declare class C { ` |
| `parser/ecmascript5/Accessors/parserAccessors6.ts` | not supported | expected `;` after expression, on ` declare class C { ` |
| `parser/ecmascript5/Accessors/parserAccessors7.ts` | not supported | expected `,` or `}`, on ` let v = { get foo(v: number) { } }; ` |
| `parser/ecmascript5/Accessors/parserAccessors8.ts` | not supported | expected `,` or `}`, on ` let v = { set foo() { } } ` |
| `parser/ecmascript5/Accessors/parserAccessors9.ts` | not supported | expected `,` or `}`, on ` let v = { set foo(a, b) { } } ` |
| `parser/ecmascript5/Accessors/parserGetAccessorWithTypeParameters1.ts` | not supported | expected `:` and a type for the class field, on ` get foo<T>(): void { } ` |
| `parser/ecmascript5/Accessors/parserSetAccessorWithTypeAnnotation1.ts` | not supported | parameter requires a type annotation, on ` set foo(v): number { ` |
| `parser/ecmascript5/Accessors/parserSetAccessorWithTypeParameters1.ts` | not supported | expected `:` and a type for the class field, on ` set foo<T>(v) { } ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression11.ts` | not supported | expected expression, on ` let v = [1,,1]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression12.ts` | not supported | expected expression, on ` let v = [1,,,1]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression13.ts` | not supported | expected expression, on ` let v = [1,,1,,1]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression14.ts` | not supported | expected expression, on ` let v = [,,1,1,,1,,1,1,,1]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression15.ts` | not supported | expected expression, on ` let v = [,,1,1,,1,,1,1,,1,]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression2.ts` | not supported | expected expression, on ` let v = [,]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression3.ts` | not supported | expected expression, on ` let v = [,,]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression4.ts` | not supported | expected expression, on ` let v = [,,,]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression6.ts` | not supported | expected expression, on ` let v = [,1]; ` |
| `parser/ecmascript5/ArrayLiteralExpressions/parserArrayLiteralExpression8.ts` | not supported | expected expression, on ` let v = [,1,]; ` |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression1.ts` | not supported | `public` is a reserved word in strict mode and can't be used as a name, on ` let v = (public x: string) => { }; ` |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression10.ts` | not supported | JavaScript |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression11.ts` | not supported | JavaScript |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression12.ts` | not supported | JavaScript |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression13.ts` | not supported | JavaScript |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression14.ts` | not supported | JavaScript |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression15.ts` | not supported | JavaScript |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression16.ts` | not supported | JavaScript |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression17.ts` | not supported | JavaScript |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression2.ts` | not supported | expected `;` after assignment, on ` a = () => { } \|\| a ` |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression3.ts` | not supported | expected `)`, on ` a = (() => { } \|\| a) ` |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression4.ts` | not supported | expected `)`, on ` a = (() => { }, a) ` |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression6.ts` | not supported | expected `)`, on ` return true ? (q ? true : false) : (b = q.length, function() { }); ` |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression7.ts` | not supported | expected `,` or `}`, on ` async m() { ` |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression8.ts` | not supported | JavaScript |
| `parser/ecmascript5/ArrowFunctionExpressions/parserArrowFunctionExpression9.ts` | not supported | JavaScript |
| `parser/ecmascript5/AutomaticSemicolonInsertion/parserAutomaticSemicolonInsertion1.ts` | not supported | expected field name in object type, on ` (): void ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration1.ts` | not supported | expected `{` after class header, on ` class C extends A extends B { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration10.ts` | not supported | expected `{`, on ` constructor(); ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration11.ts` | not supported | expected `{`, on ` constructor(); ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration12.ts` | not supported | expected `{`, on ` constructor(); ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration13.ts` | not supported | expected `:` and return type, on ` foo(); ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration14.ts` | not supported | expected `:` and return type, on ` foo(); ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration15.ts` | not supported | expected `:` and return type, on ` foo(); ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration16.ts` | not supported | expected `:` and return type, on ` foo(); ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration17.ts` | not supported | expected `;` after expression, on ` declare class Enumerator { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration18.ts` | not supported | expected `;` after expression, on ` declare class FooBase { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration19.ts` | not supported | expected `:` and return type, on ` foo(); ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration2.ts` | not supported | expected `{` after class header, on ` class C implements A implements B { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration20.ts` | not supported | expected class member name, on ` 0(); ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration21.ts` | not supported | expected class member name, on ` 0(); ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration22.ts` | not supported | expected `:` and return type, on ` "foo"(); ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration23.ts` | not supported | unexpected character `\`, on ` class C\u0032 { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration25.ts` | not supported | expected `{`, on ` data(): U; ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration26.ts` | not supported | expected `:` and a type for the class field, on ` public ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration3.ts` | not supported | expected `{` after class header, on ` class C implements A extends B { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration4.ts` | not supported | expected `{` after class header, on ` class C extends A implements B extends C { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration5.ts` | not supported | expected `{` after class header, on ` class C extends A implements B implements C { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration6.ts` | not supported | expected `{` after class header, on ` class C extends A, B { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration7.ts` | not supported | expected `;` after expression, on ` declare namespace M { ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration8.ts` | not supported | expected `{`, on ` constructor(); ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclaration9.ts` | not supported | expected `:` and return type, on ` foo(); ` |
| `parser/ecmascript5/ClassDeclarations/parserClassDeclarationIndexSignature1.ts` | not supported | index signatures |
| `parser/ecmascript5/ComputedPropertyNames/` | not supported | computed property names |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration1.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration11.ts` | not supported | expected expression, on ` } ` |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration2.ts` | not supported | a constructor cannot be `static`, on ` static constructor() { } ` |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration3.ts` | not supported | expected `:` and a type for the class field, on ` export constructor() { } ` |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration4.ts` | not supported | expected `:` and a type for the class field, on ` declare constructor() { } ` |
| `parser/ecmascript5/ConstructorDeclarations/parserConstructorDeclaration5.ts` | checks too little | 0 after the port |
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
| `parser/ecmascript5/EnumDeclarations/parserEnumDeclaration4.ts` | not supported | `void` is a reserved keyword and can't be used as a name, on ` enum void { ` |
| `parser/ecmascript5/EnumDeclarations/parserEnumDeclaration5.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/EnumDeclarations/parserEnumDeclaration6.ts` | not supported | expected `,` or `}` after enum member, on ` C = 1 << 1, ` |
| `parser/ecmascript5/EnumDeclarations/parserInterfaceKeywordInEnum.ts` | not supported | `interface` is a reserved keyword and can't be used as a name, on ` interface, ` |
| `parser/ecmascript5/EnumDeclarations/parserInterfaceKeywordInEnum1.ts` | not supported | `interface` is a reserved keyword and can't be used as a name, on ` interface, ` |
| `parser/ecmascript5/ErrorRecovery/AccessibilityAfterStatic/parserAccessibilityAfterStatic11.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/ErrorRecovery/AccessibilityAfterStatic/parserAccessibilityAfterStatic14.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/ErrorRecovery/AccessibilityAfterStatic/parserAccessibilityAfterStatic2.ts` | not supported | expected `:` and a type for the class field, on ` static public; ` |
| `parser/ecmascript5/ErrorRecovery/AccessibilityAfterStatic/parserAccessibilityAfterStatic4.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/ErrorRecovery/AccessibilityAfterStatic/parserAccessibilityAfterStatic5.ts` | not supported | expected `:` and a type for the class field, on ` } ` |
| `parser/ecmascript5/ErrorRecovery/AccessibilityAfterStatic/parserAccessibilityAfterStatic6.ts` | not supported | expected `:` and a type for the class field, on ` function main(): void {} ` |
| `parser/ecmascript5/ErrorRecovery/ArgumentLists/parserErrorRecovery_ArgumentList1.ts` | not supported | `any` is not supported, on ` function foo(): any { ` |
| `parser/ecmascript5/ErrorRecovery/ArgumentLists/parserErrorRecovery_ArgumentList2.ts` | not supported | expected expression, on ` bar(; ` |
| `parser/ecmascript5/ErrorRecovery/ArgumentLists/parserErrorRecovery_ArgumentList3.ts` | not supported | expected expression, on ` return; ` |
| `parser/ecmascript5/ErrorRecovery/ArgumentLists/parserErrorRecovery_ArgumentList4.ts` | not supported | expected `,` or `)`, on ` return; ` |
| `parser/ecmascript5/ErrorRecovery/ArgumentLists/parserErrorRecovery_ArgumentList6.ts` | not supported | expected expression, on ` Foo(, ` |
| `parser/ecmascript5/ErrorRecovery/ArgumentLists/parserErrorRecovery_ArgumentList7.ts` | not supported | expected expression, on ` Foo(a,, ` |
| `parser/ecmascript5/ErrorRecovery/ArrayLiteralExpressions/parserErrorRecoveryArrayLiteralExpression1.ts` | not supported | expected `,` or `]`, on ` 4, 5, 6, 7]; ` |
| `parser/ecmascript5/ErrorRecovery/ArrayLiteralExpressions/parserErrorRecoveryArrayLiteralExpression2.ts` | not supported | expected `,` or `]`, on ` .7042760848999023, 1.1955541372299194, 0.19600726664066315, -0.71200698614120... ` |
| `parser/ecmascript5/ErrorRecovery/ArrayLiteralExpressions/parserErrorRecoveryArrayLiteralExpression3.ts` | not supported | expected `,` or `]`, on ` let texCoords = [2, 2, 0.5000001192092895, 0.8749999 ; 403953552, 0.500000119... ` |
| `parser/ecmascript5/ErrorRecovery/ArrowFunctions/ArrowFunction1.ts` | not supported | expected type, on ` let v = (a: ) => { ` |
| `parser/ecmascript5/ErrorRecovery/ArrowFunctions/ArrowFunction3.ts` | not supported | expected `;` after declaration, on ` let v = (a): => { ` |
| `parser/ecmascript5/ErrorRecovery/ArrowFunctions/ArrowFunction4.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/ErrorRecovery/ArrowFunctions/parserX_ArrowFunction1.ts` | not supported | expected type, on ` let v = (a: ) => { ` |
| `parser/ecmascript5/ErrorRecovery/ArrowFunctions/parserX_ArrowFunction3.ts` | not supported | expected `;` after declaration, on ` let v = (a): => { ` |
| `parser/ecmascript5/ErrorRecovery/ArrowFunctions/parserX_ArrowFunction4.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/ErrorRecovery/Blocks/parserErrorRecovery_Block1.ts` | not supported | expected expression, on ` return; ` |
| `parser/ecmascript5/ErrorRecovery/Blocks/parserErrorRecovery_Block2.ts` | not supported | unexpected character `¬`, on ` ¬ ` |
| `parser/ecmascript5/ErrorRecovery/Blocks/parserErrorRecovery_Block3.ts` | not supported | expected `;` after expression, on ` private b(): boolean { ` |
| `parser/ecmascript5/ErrorRecovery/ClassElements/parserErrorRecovery_ClassElement1.ts` | not supported | expected `:` and a type for the class field, on ` class D { ` |
| `parser/ecmascript5/ErrorRecovery/ClassElements/parserErrorRecovery_ClassElement2.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `parser/ecmascript5/ErrorRecovery/ClassElements/parserErrorRecovery_ClassElement3.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `parser/ecmascript5/ErrorRecovery/Expressions/parserErrorRecovery_Expression1.ts` | not supported | expected expression, on ` let v = ()({}); ` |
| `parser/ecmascript5/ErrorRecovery/ExtendsOrImplementsClauses/parserErrorRecovery_ExtendsOrImplementsClause1.ts` | not supported | expected `{` after class header, on ` function main(): void {} ` |
| `parser/ecmascript5/ErrorRecovery/ExtendsOrImplementsClauses/parserErrorRecovery_ExtendsOrImplementsClause2.ts` | not supported | expected `{` after class header, on ` class C extends A, { ` |
| `parser/ecmascript5/ErrorRecovery/ExtendsOrImplementsClauses/parserErrorRecovery_ExtendsOrImplementsClause3.ts` | not supported | expected type, on ` class C extends implements A { ` |
| `parser/ecmascript5/ErrorRecovery/ExtendsOrImplementsClauses/parserErrorRecovery_ExtendsOrImplementsClause4.ts` | not supported | expected `{` after class header, on ` function main(): void {} ` |
| `parser/ecmascript5/ErrorRecovery/ExtendsOrImplementsClauses/parserErrorRecovery_ExtendsOrImplementsClause5.ts` | not supported | expected `{` after class header, on ` class C extends A, implements B, { ` |
| `parser/ecmascript5/ErrorRecovery/ExtendsOrImplementsClauses/parserErrorRecovery_ExtendsOrImplementsClause6.ts` | not supported | expected `{` after interface name, on ` function main(): void {} ` |
| `parser/ecmascript5/ErrorRecovery/IfStatements/parserErrorRecoveryIfStatement1.ts` | not supported | expected expression, on ` } ` |
| `parser/ecmascript5/ErrorRecovery/IfStatements/parserErrorRecoveryIfStatement2.ts` | not supported | expected `)`, on ` } ` |
| `parser/ecmascript5/ErrorRecovery/IfStatements/parserErrorRecoveryIfStatement3.ts` | not supported | expected `)`, on ` } ` |
| `parser/ecmascript5/ErrorRecovery/IfStatements/parserErrorRecoveryIfStatement4.ts` | not supported | expected expression, on ` } ` |
| `parser/ecmascript5/ErrorRecovery/IfStatements/parserErrorRecoveryIfStatement5.ts` | not supported | expected `;` after expression, on ` f2() { ` |
| `parser/ecmascript5/ErrorRecovery/IfStatements/parserErrorRecoveryIfStatement6.ts` | not supported | expected `;` after expression, on ` public f2(): void { ` |
| `parser/ecmascript5/ErrorRecovery/IncompleteMemberVariables/parserErrorRecovery_IncompleteMemberVariable1.ts` | not supported | expected `;` after expression, on ` namespace Shapes { ` |
| `parser/ecmascript5/ErrorRecovery/IncompleteMemberVariables/parserErrorRecovery_IncompleteMemberVariable2.ts` | not supported | expected `;` after expression, on ` namespace Shapes { ` |
| `parser/ecmascript5/ErrorRecovery/LeftShifts/parserErrorRecovery_LeftShift1.ts` | not supported | expected expression, on ` retValue = bfs.VARIABLES >> ); ` |
| `parser/ecmascript5/ErrorRecovery/ModuleElements/parserErrorRecovery_ModuleElement1.ts` | not supported | expected expression, on ` } ` |
| `parser/ecmascript5/ErrorRecovery/ModuleElements/parserErrorRecovery_ModuleElement2.ts` | not supported | expected expression, on ` ) ` |
| `parser/ecmascript5/ErrorRecovery/ObjectLiterals/parserErrorRecovery_ObjectLiteral1.ts` | not supported | expected `,` or `}`, on ` let v = { a: 1 b: 2 } ` |
| `parser/ecmascript5/ErrorRecovery/ObjectLiterals/parserErrorRecovery_ObjectLiteral2.ts` | not supported | expected `,` or `}`, on ` return; ` |
| `parser/ecmascript5/ErrorRecovery/ObjectLiterals/parserErrorRecovery_ObjectLiteral3.ts` | not supported | expected expression, on ` return; ` |
| `parser/ecmascript5/ErrorRecovery/ObjectLiterals/parserErrorRecovery_ObjectLiteral4.ts` | not supported | expected `,` or `}`, on ` return; ` |
| `parser/ecmascript5/ErrorRecovery/ObjectLiterals/parserErrorRecovery_ObjectLiteral5.ts` | not supported | expected `:` after field name, on ` return; ` |
| `parser/ecmascript5/ErrorRecovery/ParameterLists/parserErrorRecovery_ParameterList1.ts` | not supported | parameter requires a type annotation, on ` function f(a { ` |
| `parser/ecmascript5/ErrorRecovery/ParameterLists/parserErrorRecovery_ParameterList2.ts` | not supported | parameter requires a type annotation, on ` function f(a, { ` |
| `parser/ecmascript5/ErrorRecovery/ParameterLists/parserErrorRecovery_ParameterList4.ts` | not supported | unexpected character `¬`, on ` function f(a,¬): void { ` |
| `parser/ecmascript5/ErrorRecovery/ParameterLists/parserErrorRecovery_ParameterList5.ts` | not supported | expected `)`, on ` (a:number => { } ` |
| `parser/ecmascript5/ErrorRecovery/ParameterLists/parserErrorRecovery_ParameterList6.ts` | not supported | expected type, on ` public banana (x: break): void { } ` |
| `parser/ecmascript5/ErrorRecovery/parserCommaInTypeMemberList1.ts` | not supported | `any` is not supported, on ` let v: { workItem: any, width: string } = null as unknown as ({ workItem: any... ` |
| `parser/ecmascript5/ErrorRecovery/parserCommaInTypeMemberList2.ts` | not supported | `any` is not supported, on ` let s = $.extend< { workItem: any }, { workItem: any, width: string }>({ work... ` |
| `parser/ecmascript5/ErrorRecovery/parserEmptyParenthesizedExpression1.ts` | not supported | expected expression, on ` ().toString(); ` |
| `parser/ecmascript5/ErrorRecovery/parserEqualsGreaterThanAfterFunction1.ts` | not supported | expected function name, on ` function: any  => ` |
| `parser/ecmascript5/ErrorRecovery/parserEqualsGreaterThanAfterFunction2.ts` | not supported | expected function name, on ` function (a => b; ` |
| `parser/ecmascript5/ErrorRecovery/parserErrantAccessibilityModifierInModule1.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `parser/ecmascript5/ErrorRecovery/parserErrantEqualsGreaterThanAfterFunction1.ts` | not supported | `any` is not supported, on ` function f(): any  => 4; ` |
| `parser/ecmascript5/ErrorRecovery/parserErrantEqualsGreaterThanAfterFunction2.ts` | not supported | `any` is not supported, on ` function f(p: A): any  => p; ` |
| `parser/ecmascript5/ErrorRecovery/parserErrantSemicolonInClass1.ts` | not supported | expected `{`, on ` constructor (n: number); ` |
| `parser/ecmascript5/ErrorRecovery/parserFuzz1.ts` | not supported | expected `;` after expression, on ` cla <ss { ` |
| `parser/ecmascript5/ErrorRecovery/parserMissingLambdaOpenBrace1.ts` | not supported | expected expression, on ` let index = 0; ` |
| `parser/ecmascript5/ErrorRecovery/parserModifierOnPropertySignature1.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` public biz; ` |
| `parser/ecmascript5/ErrorRecovery/parserModifierOnPropertySignature2.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` biz; ` |
| `parser/ecmascript5/ErrorRecovery/parserModifierOnStatementInBlock1.ts` | not supported | `export` statements must appear at the top of the file, on ` export let x = this; ` |
| `parser/ecmascript5/ErrorRecovery/parserModifierOnStatementInBlock2.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/ErrorRecovery/parserModifierOnStatementInBlock3.ts` | not supported | `export` statements must appear at the top of the file, on ` export function bar(): void { ` |
| `parser/ecmascript5/ErrorRecovery/parserModifierOnStatementInBlock4.ts` | not supported | `export` statements must appear at the top of the file, on ` export function bar(): void { ` |
| `parser/ecmascript5/ErrorRecovery/parserPublicBreak1.ts` | not supported | expected `;` after expression, on ` public break; ` |
| `parser/ecmascript5/ErrorRecovery/parserStatementIsNotAMemberVariableDeclaration1.ts` | not supported | parameter `key` requires a type annotation, on ` "set": function (key, value) { ` |
| `parser/ecmascript5/ErrorRecovery/parserUnfinishedTypeNameBeforeKeyword1.ts` | not supported | expected identifier after `.` in type name, on ` let x: TypeModule1. = null as unknown as (TypeModule1.); ` |
| `parser/ecmascript5/ErrorRecovery/parserUnterminatedGeneric1.ts` | not supported | `any` is not supported, on ` all(promises: IPromise < any > []): IPromise< ` |
| `parser/ecmascript5/ErrorRecovery/parserUnterminatedGeneric2.ts` | not supported | expected `;` after expression, on ` declare namespace ng { ` |
| `parser/ecmascript5/ErrorRecovery/SourceUnits/parserErrorRecovery_SourceUnit1.ts` | not supported | expected expression, on ` } ` |
| `parser/ecmascript5/ErrorRecovery/SwitchStatements/parserErrorRecovery_SwitchStatement1.ts` | not supported | expected expression, on ` case 2: ` |
| `parser/ecmascript5/ErrorRecovery/SwitchStatements/parserErrorRecovery_SwitchStatement2.ts` | not supported | expected `case` or `default` at the start of a `switch` body, on ` class D { ` |
| `parser/ecmascript5/ErrorRecovery/TypeArgumentLists/parserX_TypeArgumentList1.ts` | not supported | unexpected character `\`, on ` Foo<A,B,\ C>(4, 5, 6); ` |
| `parser/ecmascript5/ErrorRecovery/TypeArgumentLists/TypeArgumentList1.ts` | not supported | unexpected character `\`, on ` Foo<A,B,\ C>(4, 5, 6); ` |
| `parser/ecmascript5/ErrorRecovery/VariableLists/parserErrorRecovery_VariableList1.ts` | not supported | `let` declaration requires an initializer, on ` let a, ` |
| `parser/ecmascript5/ErrorRecovery/VariableLists/parserInvalidIdentifiersInVariableStatements1.ts` | not supported | `export` is a reserved keyword and can't be used as a name, on ` let export; ` |
| `parser/ecmascript5/ErrorRecovery/VariableLists/parserVariableStatement1.ts` | not supported | `let` declaration requires an initializer, on ` let a, ` |
| `parser/ecmascript5/ErrorRecovery/VariableLists/parserVariableStatement2.ts` | not supported | `let` declaration requires an initializer, on ` , b ` |
| `parser/ecmascript5/ErrorRecovery/VariableLists/parserVariableStatement3.ts` | not supported | expected identifier after `let`/`const`, on ` a, ` |
| `parser/ecmascript5/ErrorRecovery/VariableLists/parserVariableStatement4.ts` | not supported | expected identifier after `let`/`const`, on ` a ` |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment1.ts` | not supported | expected a declaration after `export`, on ` export = foo ` |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment2.ts` | not supported | expected a declaration after `export`, on ` export = foo; ` |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment3.ts` | not supported | expected a declaration after `export`, on ` export = ` |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment4.ts` | not supported | expected a declaration after `export`, on ` export = ; ` |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment5.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment6.ts` | not supported | expected `;` after expression, on ` declare module "M" { ` |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment7.ts` | not supported | expected a declaration after `export`, on ` export = B; ` |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment8.ts` | not supported | expected a declaration after `export`, on ` export = B; ` |
| `parser/ecmascript5/ExportAssignments/parserExportAssignment9.ts` | not supported | expected `;` after expression, on ` namespace Foo { ` |
| `parser/ecmascript5/Expressions/parseIncompleteBinaryExpression1.ts` | not supported | expected expression, on ` let v = \|\| b; ` |
| `parser/ecmascript5/Expressions/parserAssignmentExpression1.ts` | not supported | invalid assignment target, on ` (foo()) = bar; ` |
| `parser/ecmascript5/Expressions/parserConditionalExpression1.ts` | not supported | expected `)`, on ` (a=this.R[c])?a.JW\|\|(a.e5(this,c),a.JW=_.l):this.A ` |
| `parser/ecmascript5/Expressions/parserMemberAccessAfterPostfixExpression1.ts` | not supported | expected `;` after expression, on ` a--.toString() ` |
| `parser/ecmascript5/Expressions/parserObjectCreation2.ts` | not supported | expected expression, on ` new new Foo()() ` |
| `parser/ecmascript5/Expressions/parserPostfixPostfixExpression1.ts` | not supported | expected `;` after expression, on ` a++ ++; ` |
| `parser/ecmascript5/Expressions/parserPostfixUnaryExpression1.ts` | not supported | expected `;` after expression, on ` foo ++ ++; ` |
| `parser/ecmascript5/Expressions/parserTypeAssertionInObjectCreationExpression1.ts` | not supported | construct signatures |
| `parser/ecmascript5/Expressions/parserUnaryExpression1.ts` | not supported | expected expression, on ` ++this; ` |
| `parser/ecmascript5/Expressions/parserUnaryExpression2.ts` | not supported | expected expression, on ` ++function(e) { } ` |
| `parser/ecmascript5/Expressions/parserUnaryExpression3.ts` | not supported | expected expression, on ` ++[0]; ` |
| `parser/ecmascript5/Expressions/parserUnaryExpression4.ts` | not supported | expected expression, on ` ++{}; ` |
| `parser/ecmascript5/Expressions/parserUnaryExpression5.ts` | not supported | expected expression, on ` ++ delete foo.bar ` |
| `parser/ecmascript5/Expressions/parserUnaryExpression6.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/Expressions/parserUnaryExpression7.ts` | not supported | expected expression, on ` ++ new Foo(); ` |
| `parser/ecmascript5/FunctionDeclarations/parserFunctionDeclaration1.ts` | not supported | expected `;` after expression, on ` declare namespace M { ` |
| `parser/ecmascript5/FunctionDeclarations/parserFunctionDeclaration2.ts` | not supported | expected expression, on ` }: void { } ` |
| `parser/ecmascript5/FunctionDeclarations/parserFunctionDeclaration3.ts` | not supported | expected `:` and return type, on ` function foo(); ` |
| `parser/ecmascript5/FunctionDeclarations/parserFunctionDeclaration4.ts` | not supported | expected `:` and return type, on ` function foo(); ` |
| `parser/ecmascript5/FunctionDeclarations/parserFunctionDeclaration5.ts` | not supported | expected `:` and return type, on ` function foo(); ` |
| `parser/ecmascript5/FunctionDeclarations/parserFunctionDeclaration6.ts` | not supported | expected `:` and return type, on ` function foo(); ` |
| `parser/ecmascript5/FunctionDeclarations/parserFunctionDeclaration7.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `parser/ecmascript5/FunctionDeclarations/parserFunctionDeclaration8.ts` | not supported | expected `;` after expression, on ` declare namespace M { ` |
| `parser/ecmascript5/Fuzz/parser0_004152.ts` | not supported | class fields require a type annotation, on ` private position = new DisplayPosition([), 3, 3, 3, 3, 3, 0, 3, 3, 3, 3, 3, 3... ` |
| `parser/ecmascript5/Fuzz/parser768531.ts` | not supported | expected `;` after expression, on ` {a: 3} ` |
| `parser/ecmascript5/Generics/parserAmbiguityWithBinaryOperator1.ts` | not supported | `let` declaration requires an initializer, on ` let a, b, c; ` |
| `parser/ecmascript5/Generics/parserAmbiguityWithBinaryOperator2.ts` | not supported | `let` declaration requires an initializer, on ` let a, b, c; ` |
| `parser/ecmascript5/Generics/parserAmbiguityWithBinaryOperator3.ts` | not supported | `let` declaration requires an initializer, on ` let a, b, c; ` |
| `parser/ecmascript5/Generics/parserAmbiguityWithBinaryOperator4.ts` | not supported | `let` declaration requires an initializer, on ` let a, b, c; ` |
| `parser/ecmascript5/Generics/parserCastVersusArrowFunction1.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `parser/ecmascript5/Generics/parserConstructorAmbiguity1.ts` | not supported | expected `(` after constructor name in `new` expression, on ` new Date<A; ` |
| `parser/ecmascript5/Generics/parserConstructorAmbiguity2.ts` | not supported | expected `(` after constructor name in `new` expression, on ` new Date<A ` |
| `parser/ecmascript5/Generics/parserConstructorAmbiguity3.ts` | not supported | expected `(` after constructor name in `new` expression, on ` function main(): void {} ` |
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
| `parser/ecmascript5/Generics/parserGenericsInInterfaceDeclaration1.ts` | not supported | construct signatures |
| `parser/ecmascript5/Generics/parserGenericsInVariableDeclaration1.ts` | not supported | expected `,` or `>` to close generic argument list, on ` let v_2 : Foo<T>= 1; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity11.ts` | not supported | invalid assignment target, on ` 1 >>= 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity12.ts` | not supported | expected expression, on ` 1 >> = 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity13.ts` | not supported | expected expression, on ` 1 >>/**/= 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity14.ts` | not supported | expected expression, on ` = 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity15.ts` | not supported | invalid assignment target, on ` 1 ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity16.ts` | not supported | invalid assignment target, on ` 1 >>>= 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity17.ts` | not supported | expected expression, on ` 1 >>> = 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity18.ts` | not supported | expected expression, on ` 1 >>>/**/= 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity19.ts` | not supported | expected expression, on ` = 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity2.ts` | not supported | expected expression, on ` 1 > > 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity20.ts` | not supported | invalid assignment target, on ` 1 ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity3.ts` | not supported | expected expression, on ` 1 >/**/> 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity4.ts` | not supported | expected expression, on ` > 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity5.ts` | duplicate | of `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity1.ts` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity6.ts` | duplicate | of `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity10.ts` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity7.ts` | not supported | expected expression, on ` 1 >> > 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity8.ts` | not supported | expected expression, on ` 1 >>/**/> 2; ` |
| `parser/ecmascript5/Generics/parserGreaterThanTokenAmbiguity9.ts` | not supported | expected expression, on ` > 2; ` |
| `parser/ecmascript5/Generics/parserMemberAccessExpression1.ts` | not supported | expected expression, on ` Foo<T>.Bar(); ` |
| `parser/ecmascript5/Generics/parserMemberAccessOffOfGenericType1.ts` | not supported | expected expression, on ` let v = List<number>.makeChild(); ` |
| `parser/ecmascript5/IndexMemberDeclarations/` | not supported | index signatures |
| `parser/ecmascript5/IndexSignatures/` | not supported | index signatures |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration1.ts` | not supported | expected `{` after interface name, on ` interface I extends A extends B { ` |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration2.ts` | not supported | expected `{` after interface name, on ` interface I implements A { ` |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration3.ts` | not supported | expected `;` after expression, on ` public interface I { ` |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration4.ts` | not supported | expected `;` after expression, on ` static interface I { ` |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration5.ts` | not supported | expected `;` after expression, on ` declare interface I { ` |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration6.ts` | not supported | expected a declaration after `export`, on ` export export interface I { ` |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration7.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/InterfaceDeclarations/parserInterfaceDeclaration9.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` get foo(): number, ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessor1.ts` | not supported | parameter requires a type annotation, on ` set foo(a) { } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration10.ts` | not supported | expected `:` and a type for the class field, on ` export get Foo(): void { } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration11.ts` | not supported | expected `:` and a type for the class field, on ` declare get Foo(): void { } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration13.ts` | not supported | expected expression, on ` } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration14.ts` | not supported | expected expression, on ` } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration18.ts` | not supported | rest parameter requires a type annotation, on ` set Foo(...a) { } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration2.ts` | not supported | expected `:` and a type for the class field, on ` get "b"(): void { } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration3.ts` | not supported | expected `:` and a type for the class field, on ` get 0(): void { } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration4.ts` | not supported | parameter requires a type annotation, on ` set a(i) { } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration5.ts` | not supported | expected `:` and a type for the class field, on ` set "a"(i) { } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration6.ts` | not supported | expected `:` and a type for the class field, on ` set 0(i) { } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration8.ts` | not supported | static accessors are not supported, on ` static static get Foo(): void { } ` |
| `parser/ecmascript5/MemberAccessorDeclarations/parserMemberAccessorDeclaration9.ts` | not supported | static accessors are not supported, on ` static public get Foo(): void { } ` |
| `parser/ecmascript5/MemberFunctionDeclarations/parserMemberFunctionDeclaration2.ts` | the port changes what it checks | `tsc` then reports TS7008 |
| `parser/ecmascript5/MemberFunctionDeclarations/parserMemberFunctionDeclaration4.ts` | not supported | expected `:` and a type for the class field, on ` export Foo(): void { } ` |
| `parser/ecmascript5/MemberFunctionDeclarations/parserMemberFunctionDeclaration5.ts` | not supported | expected `:` and a type for the class field, on ` declare Foo(): void { } ` |
| `parser/ecmascript5/MemberVariableDeclarations/parserMemberVariableDeclaration1.ts` | not supported | expected `:` and a type for the class field, on ` public public Foo; ` |
| `parser/ecmascript5/MemberVariableDeclarations/parserMemberVariableDeclaration2.ts` | not supported | expected `:` and a type for the class field, on ` static static Foo; ` |
| `parser/ecmascript5/MemberVariableDeclarations/parserMemberVariableDeclaration3.ts` | not supported | expected `:` and a type for the class field, on ` static public Foo; ` |
| `parser/ecmascript5/MemberVariableDeclarations/parserMemberVariableDeclaration4.ts` | not supported | expected `:` and a type for the class field, on ` export Foo; ` |
| `parser/ecmascript5/MemberVariableDeclarations/parserMemberVariableDeclaration5.ts` | not supported | expected `:` and a type for the class field, on ` declare Foo; ` |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature1.ts` | not supported | expected `:` and return type, on ` A(); ` |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature10.ts` | not supported | expected interface member name, on ` 1?(); ` |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature11.ts` | not supported | expected interface member name, on ` 2<T>(); ` |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature12.ts` | not supported | expected interface member name, on ` 3?<T>(); ` |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature2.ts` | not supported | expected `:` and return type, on ` B?(); ` |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature3.ts` | not supported | expected `:` and return type, on ` C<T>(); ` |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature4.ts` | not supported | expected `:` and return type, on ` D?<T>(); ` |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature5.ts` | not supported | expected `:` and return type, on ` "E"(); ` |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature6.ts` | not supported | expected `:` and return type, on ` "F"?(); ` |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature7.ts` | not supported | expected `:` and return type, on ` "G"<T>(); ` |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature8.ts` | not supported | expected `:` and return type, on ` "H"?<T>(); ` |
| `parser/ecmascript5/MethodSignatures/parserMethodSignature9.ts` | not supported | expected interface member name, on ` 0(); ` |
| `parser/ecmascript5/MissingTokens/parserMissingToken1.ts` | not supported | expected expression, on ` a / finally ` |
| `parser/ecmascript5/MissingTokens/parserMissingToken2.ts` | not supported | unterminated regex literal, on ` / b; ` |
| `parser/ecmascript5/ModuleDeclarations/parserModule1.ts` | not supported | expected a declaration after `export`, on ` export namespace CompilerDiagnostics { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration1.ts` | not supported | expected `;` after expression, on ` module "Foo" { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration10.ts` | not supported | expected `{`, on ` function data(): string; ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration11.ts` | not supported | expected `;` after expression, on ` declare namespace string { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration12.ts` | not supported | expected `;` after expression, on ` namespace A.string { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration2.ts` | not supported | expected `;` after expression, on ` declare module "Foo" { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration3.ts` | not supported | expected `;` after expression, on ` declare namespace M { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration4.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration5.ts` | not supported | expected `;` after expression, on ` namespace M1 { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration6.ts` | not supported | expected `;` after expression, on ` namespace number { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration7.ts` | not supported | expected `;` after expression, on ` namespace number.a { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration8.ts` | not supported | expected `;` after expression, on ` namespace a.number { ` |
| `parser/ecmascript5/ModuleDeclarations/parserModuleDeclaration9.ts` | not supported | expected `;` after expression, on ` namespace a.number.b { ` |
| `parser/ecmascript5/ObjectTypes/parserObjectType3.ts` | not supported | expected `:` after field name, on ` x; ` |
| `parser/ecmascript5/ObjectTypes/parserObjectType4.ts` | not supported | expected `:` after field name, on ` y ` |
| `parser/ecmascript5/ObjectTypes/parserObjectType5.ts` | not supported | expected field name in object type, on ` <T>; ` |
| `parser/ecmascript5/ParameterLists/parserParameterList1.ts` | not supported | rest parameter requires a type annotation, on ` F(...A, B): void { } ` |
| `parser/ecmascript5/ParameterLists/parserParameterList10.ts` | not supported | expected expression, on ` } ` |
| `parser/ecmascript5/ParameterLists/parserParameterList11.ts` | not supported | a rest parameter cannot be optional, on ` (...arg?) => 102; ` |
| `parser/ecmascript5/ParameterLists/parserParameterList12.ts` | not supported | parameter requires a type annotation, on ` function F(a,): void { ` |
| `parser/ecmascript5/ParameterLists/parserParameterList13.ts` | not supported | construct signatures |
| `parser/ecmascript5/ParameterLists/parserParameterList14.ts` | not supported | expected `;` after expression, on ` declare class C { ` |
| `parser/ecmascript5/ParameterLists/parserParameterList15.ts` | not supported | expected `:` and return type, on ` function foo(a: number = 4); ` |
| `parser/ecmascript5/ParameterLists/parserParameterList16.ts` | not supported | expected `:` and return type, on ` foo(a: number = 4); ` |
| `parser/ecmascript5/ParameterLists/parserParameterList17.ts` | not supported | expected `{`, on ` constructor(a: number = 4); ` |
| `parser/ecmascript5/ParameterLists/parserParameterList3.ts` | not supported | parameter requires a type annotation, on ` F(A?, B): void { } ` |
| `parser/ecmascript5/ParameterLists/parserParameterList4.ts` | not supported | parameter requires a type annotation, on ` function F(public A): void { ` |
| `parser/ecmascript5/ParameterLists/parserParameterList5.ts` | not supported | `public` is a reserved word in strict mode and can't be used as a name, on ` function A(): (public B) => C { ` |
| `parser/ecmascript5/ParameterLists/parserParameterList6.ts` | not supported | `public` is a reserved word in strict mode and can't be used as a name, on ` constructor(C: (public A) => any) { ` |
| `parser/ecmascript5/ParameterLists/parserParameterList7.ts` | not supported | expected `{`, on ` constructor(public p1:string); // ERROR ` |
| `parser/ecmascript5/ParameterLists/parserParameterList8.ts` | not supported | expected `;` after expression, on ` declare class C2 { ` |
| `parser/ecmascript5/ParameterLists/parserParameterList9.ts` | not supported | a rest parameter cannot be optional, on ` foo(...bar?): void { } ` |
| `parser/ecmascript5/parser10.1.1-8gs.ts` | not supported | `public` is a reserved word in strict mode and can't be used as a name, on ` let public = 1; ` |
| `parser/ecmascript5/parserArgumentList1.ts` | not supported | unknown type `HTMLElement`, on ` export function removeClass (node:HTMLElement, className:string): void { ` |
| `parser/ecmascript5/parserAstSpans1.ts` | not supported | expected `:` and a type for the class field, on ` public i1_p1!: number; ` |
| `parser/ecmascript5/parserDebuggerStatement1.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/parserDebuggerStatement2.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/parserEmptyFile1.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/parserExportAsFunctionIdentifier.ts` | not supported | `as` to `Foo` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let f: Foo = null as unknown as (Foo); ` |
| `parser/ecmascript5/parserImportDeclaration1.ts` | not supported | expected `from` after import specifier list, on ` import TypeScript = TypeScriptServices.TypeScript; ` |
| `parser/ecmascript5/parserKeywordsAsIdentifierName2.ts` | not supported | unterminated block comment, on ` a.public /* ` |
| `parser/ecmascript5/parserNoASIOnCallAfterFunctionExpression1.ts` | not supported | `any` is not supported, on ` (<any>window).foo; ` |
| `parser/ecmascript5/parserNotRegex2.ts` | not supported | `any` is not supported, on ` const A: any = null as unknown as (any); ` |
| `parser/ecmascript5/parserObjectCreationArrayLiteral1.ts` | not supported | expected `(` after constructor name in `new` expression, on ` new Foo[]; ` |
| `parser/ecmascript5/parserObjectCreationArrayLiteral2.ts` | not supported | expected `(` after constructor name in `new` expression, on ` new Foo[1]; ` |
| `parser/ecmascript5/parserObjectCreationArrayLiteral3.ts` | not supported | expected `(` after constructor name in `new` expression, on ` new Foo[](); ` |
| `parser/ecmascript5/parserObjectCreationArrayLiteral4.ts` | not supported | expected `(` after constructor name in `new` expression, on ` new Foo[1](); ` |
| `parser/ecmascript5/parserOptionalTypeMembers1.ts` | not supported | `any` is not supported, on ` value?: any; ` |
| `parser/ecmascript5/parserOverloadOnConstants1.ts` | not supported | unknown type `HTMLElement`, on ` createElement(tagName: string): HTMLElement; ` |
| `parser/ecmascript5/parserParenthesizedVariableAndFunctionInTernary.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `parser/ecmascript5/parserParenthesizedVariableAndParenthesizedFunctionInTernary.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `parser/ecmascript5/parserRealSource1.ts` | not supported | expected `;` after expression, on ` namespace TypeScript { ` |
| `parser/ecmascript5/parserRealSource10.ts` | not supported | expected `;` after expression, on ` namespace TypeScript { ` |
| `parser/ecmascript5/parserRealSource11.ts` | not supported | expected `;` after expression, on ` namespace TypeScript { ` |
| `parser/ecmascript5/parserRealSource12.ts` | not supported | expected `;` after expression, on ` namespace TypeScript { ` |
| `parser/ecmascript5/parserRealSource13.ts` | not supported | expected `;` after expression, on ` namespace TypeScript.AstWalkerWithDetailCallback { ` |
| `parser/ecmascript5/parserRealSource14.ts` | not supported | expected `;` after expression, on ` namespace TypeScript { ` |
| `parser/ecmascript5/parserRealSource2.ts` | not supported | expected `;` after expression, on ` namespace TypeScript { ` |
| `parser/ecmascript5/parserRealSource3.ts` | not supported | expected `;` after expression, on ` namespace TypeScript { ` |
| `parser/ecmascript5/parserRealSource4.ts` | not supported | expected `;` after expression, on ` namespace TypeScript { ` |
| `parser/ecmascript5/parserRealSource5.ts` | not supported | expected `;` after expression, on ` namespace TypeScript { ` |
| `parser/ecmascript5/parserRealSource6.ts` | not supported | expected `;` after expression, on ` namespace TypeScript { ` |
| `parser/ecmascript5/parserRealSource7.ts` | not supported | expected `;` after expression, on ` namespace TypeScript { ` |
| `parser/ecmascript5/parserRealSource8.ts` | not supported | expected `;` after expression, on ` namespace TypeScript { ` |
| `parser/ecmascript5/parserRealSource9.ts` | not supported | expected `;` after expression, on ` namespace TypeScript { ` |
| `parser/ecmascript5/parserS12.11_A3_T4.ts` | not supported | parameter requires a type annotation, on ` function SwitchTest(value): number { ` |
| `parser/ecmascript5/parserS7.3_A1.1_T2.ts` | not supported | expected identifier after `let`/`const`, on ` x ` |
| `parser/ecmascript5/parserS7.6_A4.2_T1.ts` | not supported | unexpected character `\`, on ` let \u0410 = 1; ` |
| `parser/ecmascript5/parserS7.6.1.1_A1.10.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/parserS7.9_A5.7_T1.ts` | not supported | expected `;` after declaration, on ` let x=0, y=0; ` |
| `parser/ecmascript5/parserSbp_7.9_A9_T3.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/parserSyntaxWalker.generated.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/parserUnicode1.ts` | not supported | unexpected character `\`, on ` let \u0078x = 6; ` |
| `parser/ecmascript5/parserUnicode3.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/parserUnicodeWhitespaceCharacter1.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/parserUsingConstructorAsIdentifier.ts` | not supported | parameter requires a type annotation, on ` function define(constructor, instanceMembers, staticMembers): any { ` |
| `parser/ecmascript5/parservoidInQualifiedName1.ts` | not supported | expected `;` after declaration, on ` let v : void = null as unknown as (void).x; ` |
| `parser/ecmascript5/parservoidInQualifiedName2.ts` | not supported | expected identifier after `.` in type name, on ` let v : x.void = null as unknown as (x.void); ` |
| `parser/ecmascript5/PropertyAssignments/parserFunctionPropertyAssignment2.ts` | not supported | expected field name, on ` let v = { 0() { } }; ` |
| `parser/ecmascript5/PropertyAssignments/parserFunctionPropertyAssignment4.ts` | not supported | expected field name, on ` let v = { 0<T>() { } }; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature1.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` A; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature10.ts` | not supported | expected interface member name, on ` 1?; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature11.ts` | not supported | expected interface member name, on ` 2:any; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature12.ts` | not supported | expected interface member name, on ` 3?:any; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature2.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` B?; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature3.ts` | not supported | `any` is not supported, on ` C:any; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature4.ts` | not supported | `any` is not supported, on ` D?:any; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature5.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` "E"; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature6.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` "F"?; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature7.ts` | not supported | `any` is not supported, on ` "G":any; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature8.ts` | not supported | `any` is not supported, on ` "H"?:any; ` |
| `parser/ecmascript5/PropertySignatures/parserPropertySignature9.ts` | not supported | expected interface member name, on ` 0; ` |
| `parser/ecmascript5/Protected/Protected1.ts` | not supported | expected `;` after expression, on ` protected class C { ` |
| `parser/ecmascript5/Protected/Protected2.ts` | not supported | expected `;` after expression, on ` protected namespace M { ` |
| `parser/ecmascript5/Protected/Protected3.ts` | not supported | `protected` is not supported, on ` protected constructor() { } ` |
| `parser/ecmascript5/Protected/Protected4.ts` | not supported | `protected` is not supported, on ` protected public m(): void { } ` |
| `parser/ecmascript5/Protected/Protected5.ts` | not supported | `protected` is not supported, on ` protected static m(): void { } ` |
| `parser/ecmascript5/Protected/Protected6.ts` | not supported | expected `:` and a type for the class field, on ` static protected m(): void { } ` |
| `parser/ecmascript5/Protected/Protected7.ts` | not supported | `protected` is not supported, on ` protected private m(): void { } ` |
| `parser/ecmascript5/Protected/Protected8.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` p ` |
| `parser/ecmascript5/Protected/Protected9.ts` | not supported | `protected` is a reserved word in strict mode and can't be used as a name, on ` constructor(protected p) { } ` |
| `parser/ecmascript5/RealWorld/parserharness.ts` | not supported | construct signatures |
| `parser/ecmascript5/RealWorld/parserindenter.ts` | not supported | expected `;` after expression, on ` namespace Formatting { ` |
| `parser/ecmascript5/RegressionTests/parser509534.ts` | the port changes what it checks | `tsc` then reports TS2580, TS7006 |
| `parser/ecmascript5/RegressionTests/parser509546_1.ts` | not supported | expected `:` and a type for the class field, on ` } ` |
| `parser/ecmascript5/RegressionTests/parser509546_2.ts` | not supported | expected `:` and a type for the class field, on ` } ` |
| `parser/ecmascript5/RegressionTests/parser509546.ts` | not supported | expected `:` and a type for the class field, on ` } ` |
| `parser/ecmascript5/RegressionTests/parser509618.ts` | not supported | expected `;` after expression, on ` declare namespace ambiModule { ` |
| `parser/ecmascript5/RegressionTests/parser509630.ts` | not supported | unknown type `Type`, on ` class Any extends Type { ` |
| `parser/ecmascript5/RegressionTests/parser509667.ts` | not supported | expected field name after `.`, on ` } ` |
| `parser/ecmascript5/RegressionTests/parser509668.ts` | not supported | `public` is a reserved word in strict mode and can't be used as a name, on ` constructor (public ...args: string[]) { } ` |
| `parser/ecmascript5/RegressionTests/parser509669.ts` | not supported | `any` is not supported, on ` function foo():any { ` |
| `parser/ecmascript5/RegressionTests/parser509693.ts` | the port changes what it checks | `tsc` then reports TS2580 |
| `parser/ecmascript5/RegressionTests/parser509698.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/RegressionTests/parser512084.ts` | not supported | expected `:` and a type for the class field, on ` function main(): void {} ` |
| `parser/ecmascript5/RegressionTests/parser512097.ts` | not supported | expected `,` or `}`, on ` let tt = { aa; } ` |
| `parser/ecmascript5/RegressionTests/parser512325.ts` | not supported | expected parameter name, on ` let tt = (a, (b, c)) => a+b+c; ` |
| `parser/ecmascript5/RegressionTests/parser519458.ts` | not supported | expected `from` after import specifier list, on ` import rect = module("rect"); let bar = new rect.Rect(); ` |
| `parser/ecmascript5/RegressionTests/parser521128.ts` | not supported | expected `;` after expression, on ` module.module { } ` |
| `parser/ecmascript5/RegressionTests/parser566700.ts` | not supported | expected expression, on ` let v = ()({}); ` |
| `parser/ecmascript5/RegressionTests/parser585151.ts` | not supported | expected `:` and a type for the class field, on ` let icecream = "chocolate"; ` |
| `parser/ecmascript5/RegressionTests/parser618973.ts` | not supported | expected a declaration after `export`, on ` export export class Foo { ` |
| `parser/ecmascript5/RegressionTests/parser642331_1.ts` | not supported | `static` is a reserved word in strict mode and can't be used as a name, on ` constructor (static) { } ` |
| `parser/ecmascript5/RegressionTests/parser642331.ts` | not supported | `static` is a reserved word in strict mode and can't be used as a name, on ` constructor (static) { } ` |
| `parser/ecmascript5/RegressionTests/parser643728.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` foo; ` |
| `parser/ecmascript5/RegressionTests/parser645086_1.ts` | not supported | expected `;` after declaration, on ` let v = /[]/]/ ` |
| `parser/ecmascript5/RegressionTests/parser645086_2.ts` | not supported | expected `;` after declaration, on ` let v = /[^]/]/ ` |
| `parser/ecmascript5/RegressionTests/parser645484.ts` | not supported | `any` is not supported, on ` new?(): any; ` |
| `parser/ecmascript5/RegressionTests/parserTernaryAndCommaOperators1.ts` | not supported | expected `;` after expression, on ` b.src ? 1 : 2, c && d; ` |
| `parser/ecmascript5/RegularExpressions/parserRegularExpression4.ts` | not supported | unexpected character `\`, on ` if (Ca.test(c.href) \|\| Ba.test(c.href) && /(\\?\|&)adurl=/.test(c.href) && !/(... ` |
| `parser/ecmascript5/RegularExpressions/parserRegularExpression5.ts` | not supported | unexpected character `\`, on ` if (a) / (\\ ? \| & ) rct = j / .test(c.href); ` |
| `parser/ecmascript5/RegularExpressions/parserRegularExpression6.ts` | not supported | `let` declaration requires an initializer, on ` let a; ` |
| `parser/ecmascript5/RegularExpressions/parserRegularExpressionDivideAmbiguity3.ts` | not supported | expected expression, on ` if (1) /regexp/a.foo(); ` |
| `parser/ecmascript5/RegularExpressions/parserRegularExpressionDivideAmbiguity4.ts` | not supported | unterminated regex literal, on ` foo(/notregexp); ` |
| `parser/ecmascript5/RegularExpressions/parserRegularExpressionDivideAmbiguity6.ts` | not supported | parameter requires a type annotation, on ` function c255lsqr8h(a7, a6, a5, a4, a3, a2, a1, a0): number[] { ` |
| `parser/ecmascript5/RegularExpressions/parserRegularExpressionDivideAmbiguity7.ts` | not supported | expected `;` after expression, on ` ){} ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens1.ts` | not supported | unexpected character `\`, on ` \ ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens10.ts` | not supported | unexpected character `\`, on ` \ ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens11.ts` | not supported | unexpected character `\`, on ` ; \ \ \ ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens12.ts` | not supported | unexpected character `\`, on ` \ \ \ ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens13.ts` | not supported | unexpected character `\`, on ` /regexp/ \ ; ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens14.ts` | not supported | unexpected character `\`, on ` \ ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens15.ts` | not supported | unexpected character `\`, on ` \ ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens16.ts` | not supported | unexpected character `¬`, on ` function Foo      (): any  ¬   { } ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens17.ts` | not supported | unexpected character `\`, on ` foo(a, \ ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens18.ts` | not supported | unexpected character `\`, on ` foo(a \ ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens19.ts` | not supported | unexpected character `\`, on ` \ let v; ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens2.ts` | not supported | unexpected character `\`, on ` \\ ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens20.ts` | not supported | unexpected character `\`, on ` let v: X<T = null as unknown as (X<T) \; ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens3.ts` | not supported | unexpected character `\`, on ` \ ; \ ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens4.ts` | not supported | unexpected character `\`, on ` \  /regexp/; ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens5.ts` | not supported | unexpected character `\`, on ` \ /*foo*/ ; ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens6.ts` | not supported | unexpected character `\`, on ` /*foo*/ \ ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens7.ts` | not supported | unexpected character `\`, on ` /*foo*/ \ /*bar*/ ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens8.ts` | not supported | unexpected character `\`, on ` /*foo*/ \ /*bar*/ ` |
| `parser/ecmascript5/SkippedTokens/parserSkippedTokens9.ts` | not supported | unexpected character `\`, on ` /*foo*/ \ /*bar*/ ` |
| `parser/ecmascript5/Statements/BreakStatements/parser_breakInIterationOrSwitchStatement1.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/Statements/BreakStatements/parser_breakInIterationOrSwitchStatement2.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/Statements/BreakStatements/parser_breakInIterationOrSwitchStatement3.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/Statements/BreakStatements/parser_breakInIterationOrSwitchStatement4.ts` | not supported | `let` declaration requires an initializer, on ` for (let i in something) { ` |
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
| `parser/ecmascript5/Statements/parserES5ForOfStatement12.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (const {a, b} of X) { ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement13.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (let {a, b} of X) { ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement15.ts` | duplicate | of `parser/ecmascript5/Statements/parserES5ForOfStatement14.ts` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement16.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (let {a, b} of X) { ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement17.ts` | not supported | `let` declaration requires an initializer, on ` for (let of; ;) { } ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement18.ts` | the port changes what it checks | `tsc` then reports TS7022, TS2448 |
| `parser/ecmascript5/Statements/parserES5ForOfStatement19.ts` | not supported | `let` declaration requires an initializer, on ` for (let of in of) { } ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement2.ts` | not supported | `let` declaration requires an initializer, on ` for (let of X) { ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement20.ts` | not supported | expected `;` after declaration, on ` for (let of = 0 in of) { } ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement21.ts` | not supported | expected expression, on ` for (let of of) { } ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement3.ts` | not supported | `let` declaration requires an initializer, on ` for (let a, b of X) { ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement4.ts` | not supported | expected `;` after declaration, on ` for (let a = 1 of X) { ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement6.ts` | not supported | expected `;` after declaration, on ` for (let a = 1, b = 2 of X) { ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement7.ts` | not supported | expected `;` after declaration, on ` for (let a: number = 1, b: string = "" of X) { ` |
| `parser/ecmascript5/Statements/parserES5ForOfStatement9.ts` | duplicate | of `parser/ecmascript5/Statements/parserES5ForOfStatement8.ts` |
| `parser/ecmascript5/Statements/parserForInStatement2.ts` | not supported | `in` is a reserved keyword and can't be used as a name, on ` for (let in X) { ` |
| `parser/ecmascript5/Statements/parserForInStatement3.ts` | not supported | `let` declaration requires an initializer, on ` for (let a, b in X) { ` |
| `parser/ecmascript5/Statements/parserForInStatement4.ts` | not supported | expected `;` after declaration, on ` for (let a = 1 in X) { ` |
| `parser/ecmascript5/Statements/parserForInStatement5.ts` | not supported | `let` declaration requires an initializer, on ` for (let a: number in X) { ` |
| `parser/ecmascript5/Statements/parserForInStatement6.ts` | not supported | expected `;` after declaration, on ` for (let a = 1, b = 2 in X) { ` |
| `parser/ecmascript5/Statements/parserForInStatement7.ts` | not supported | expected `;` after declaration, on ` for (let a: number = 1, b: string = "" in X) { ` |
| `parser/ecmascript5/Statements/parserForInStatement8.ts` | not supported | `let` declaration requires an initializer, on ` for (let [x = 'a' in {}] in { '': 0 }) console.log(x) ` |
| `parser/ecmascript5/Statements/parserForStatement2.ts` | not supported | `let` declaration requires an initializer, on ` let a; ` |
| `parser/ecmascript5/Statements/parserForStatement3.ts` | not supported | invalid assignment target, on ` for(d in _.jh[a]=_.jh[a]\|\|[],b); ` |
| `parser/ecmascript5/Statements/parserForStatement4.ts` | not supported | expected `;` after assignment, on ` for (a = 1 in b) { ` |
| `parser/ecmascript5/Statements/parserForStatement5.ts` | not supported | expected `;` after expression, on ` for ({} in b) { ` |
| `parser/ecmascript5/Statements/parserForStatement6.ts` | not supported | expected `;` after expression, on ` for (foo() in b) { ` |
| `parser/ecmascript5/Statements/parserForStatement7.ts` | not supported | expected `;` after expression, on ` for (new foo() in b) { ` |
| `parser/ecmascript5/Statements/parserForStatement8.ts` | not supported | expected `;` after expression, on ` for (this in b) { ` |
| `parser/ecmascript5/Statements/ReturnStatements/parserReturnStatement3.ts` | checks too little | 0 after the port |
| `parser/ecmascript5/Statements/ReturnStatements/parserReturnStatement4.ts` | not supported | expected `,` or `}`, on ` let v = { get foo() { return } }; ` |
| `parser/ecmascript5/StrictMode/parserStrictMode10.ts` | not supported | parameter requires a type annotation, on ` function f(eval): void { ` |
| `parser/ecmascript5/StrictMode/parserStrictMode11.ts` | the port changes what it checks | `tsc` then reports TS7006 |
| `parser/ecmascript5/StrictMode/parserStrictMode12.ts` | not supported | expected `,` or `}`, on ` let v = { set foo(eval) { } } ` |
| `parser/ecmascript5/StrictMode/parserStrictMode14.ts` | not supported | expected `;` after expression, on ` with (a) { ` |
| `parser/ecmascript5/StrictMode/parserStrictMode15-negative.ts` | not supported | the `delete` operator is not supported, on ` delete a[b]; ` |
| `parser/ecmascript5/StrictMode/parserStrictMode15.ts` | not supported | the `delete` operator is not supported, on ` delete a; ` |
| `parser/ecmascript5/StrictMode/parserStrictMode16.ts` | not supported | expected `;` after expression, on ` delete 1; ` |
| `parser/ecmascript5/StrictMode/parserStrictMode7.ts` | not supported | expected expression, on ` ++eval; ` |
| `parser/ecmascript5/SuperExpressions/parserSuperExpression1.ts` | not supported | expected `;` after expression, on ` namespace M1.M2 { ` |
| `parser/ecmascript5/SuperExpressions/parserSuperExpression4.ts` | not supported | expected `;` after expression, on ` namespace M1.M2 { ` |
| `parser/ecmascript5/Symbols/` | not supported | `Symbol` |
| `parser/ecmascript5/TupleTypes/TupleType3.ts` | not supported | tuple types must have at least one element, on ` let v: [] = null as unknown as ([]); ` |
| `parser/ecmascript5/TupleTypes/TupleType4.ts` | not supported | expected type, on ` let v: [ = null as unknown as ([); ` |
| `parser/ecmascript5/TupleTypes/TupleType6.ts` | not supported | expected type, on ` let v: [number,,] = null as unknown as ([number,,]); ` |
| `parser/ecmascript5/Types/parserTypeQuery2.ts` | duplicate | of `parser/ecmascript5/Types/parserTypeQuery1.ts` |
| `parser/ecmascript5/Types/parserTypeQuery3.ts` | not supported | expected a property name after `.`, on ` let v: typeof A. = null as unknown as (typeof A.); ` |
| `parser/ecmascript5/Types/parserTypeQuery4.ts` | not supported | expected a property name after `.`, on ` let v: typeof A. = null as unknown as (typeof A.); ` |
| `parser/ecmascript5/Types/parserTypeQuery6.ts` | duplicate | of `parser/ecmascript5/Types/parserTypeQuery5.ts` |
| `parser/ecmascript5/Types/parserTypeQuery8.ts` | not supported | `let` declaration requires an initializer, on ` let v: typeof A<B> = null as unknown as (typeof A<B>); ` |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration1.ts` | not supported | expected `;` after declaration, on ` let selection = a, ` |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration10.ts` | not supported | `let` declaration requires an initializer, on ` let a,; ` |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration11.ts` | not supported | `let` declaration requires an initializer, on ` let a,b; ` |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration3.ts` | not supported | expected expression, on ` , outerr = new Harness.Compiler.WriterAggregator() ` |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration4.ts` | not supported | expected `;` after expression, on ` declare namespace M { ` |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration5.ts` | not supported | `let` declaration requires an initializer, on ` let a, ` |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration6.ts` | not supported | expected identifier after `let`/`const`, on ` function main(): void {} ` |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration7.ts` | not supported | `let` declaration requires an initializer, on ` let a,b ` |
| `parser/ecmascript5/VariableDeclarations/parserVariableDeclaration8.ts` | not supported | expected identifier after `let`/`const`, on ` let ; ` |
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
| `parser/ecmascript6/Iterators/parserForOfStatement19.ts` | not supported | `let` declaration requires an initializer, on ` for (let of in of) { } ` |
| `parser/ecmascript6/Iterators/parserForOfStatement2.ts` | not supported | `let` declaration requires an initializer, on ` for (let of X) { ` |
| `parser/ecmascript6/Iterators/parserForOfStatement20.ts` | not supported | expected `;` after declaration, on ` for (let of = 0 in of) { } ` |
| `parser/ecmascript6/Iterators/parserForOfStatement21.ts` | not supported | expected expression, on ` for (let of of) { } ` |
| `parser/ecmascript6/Iterators/parserForOfStatement22.ts` | not supported | `let` declaration requires an initializer, on ` let async; ` |
| `parser/ecmascript6/Iterators/parserForOfStatement23.ts` | not supported | expected `;` after expression, on ` async function foo(x: any): Promise<void> { ` |
| `parser/ecmascript6/Iterators/parserForOfStatement24.ts` | not supported | `let` declaration requires an initializer, on ` let async; ` |
| `parser/ecmascript6/Iterators/parserForOfStatement25.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (let {x = 'a' in {}} of [{}]) console.log(x) ` |
| `parser/ecmascript6/Iterators/parserForOfStatement3.ts` | not supported | `let` declaration requires an initializer, on ` for (let a, b of X) { ` |
| `parser/ecmascript6/Iterators/parserForOfStatement4.ts` | not supported | expected `;` after declaration, on ` for (let a = 1 of X) { ` |
| `parser/ecmascript6/Iterators/parserForOfStatement5.ts` | duplicate | of `parser/ecmascript5/Statements/parserES5ForOfStatement5.ts` |
| `parser/ecmascript6/Iterators/parserForOfStatement6.ts` | not supported | expected `;` after declaration, on ` for (let a = 1, b = 2 of X) { ` |
| `parser/ecmascript6/Iterators/parserForOfStatement7.ts` | not supported | expected `;` after declaration, on ` for (let a: number = 1, b: string = "" of X) { ` |
| `parser/ecmascript6/Iterators/parserForOfStatement8.ts` | duplicate | of `parser/ecmascript5/Statements/parserES5ForOfStatement8.ts` |
| `parser/ecmascript6/Iterators/parserForOfStatement9.ts` | duplicate | of `parser/ecmascript5/Statements/parserES5ForOfStatement8.ts` |
| `parser/ecmascript6/ShorthandPropertyAssignment/parserShorthandPropertyAssignment1.ts` | not supported | `any` is not supported, on ` let name:any = null as unknown as (any), id: any = null as unknown as (any); ` |
| `parser/ecmascript6/ShorthandPropertyAssignment/parserShorthandPropertyAssignment2.ts` | not supported | expected `:` after field name, on ` let v = { class }; ` |
| `parser/ecmascript6/ShorthandPropertyAssignment/parserShorthandPropertyAssignment3.ts` | not supported | expected `:` after field name, on ` let v = { "" }; ` |
| `parser/ecmascript6/ShorthandPropertyAssignment/parserShorthandPropertyAssignment4.ts` | not supported | expected field name, on ` let v = { 0 }; ` |
| `parser/ecmascript6/ShorthandPropertyAssignment/parserShorthandPropertyAssignment5.ts` | not supported | expected `,` or `}`, on ` let obj = { greet? }; ` |
| `parser/ecmascript6/Symbols/` | not supported | `Symbol` |
| `references/library-reference-1.ts` | not supported | declaration files (`.d.ts`) |
| `references/library-reference-10.ts` | not supported | JSON modules |
| `references/library-reference-11.ts` | not supported | JavaScript |
| `references/library-reference-12.ts` | not supported | `node_modules` packages |
| `references/library-reference-13.ts` | not supported | JSON modules |
| `references/library-reference-14.ts` | not supported | declaration files (`.d.ts`) |
| `references/library-reference-15.ts` | not supported | declaration files (`.d.ts`) |
| `references/library-reference-2.ts` | not supported | JSON modules |
| `references/library-reference-3.ts` | not supported | `node_modules` packages |
| `references/library-reference-4.ts` | not supported | `node_modules` packages |
| `references/library-reference-5.ts` | not supported | `node_modules` packages |
| `references/library-reference-6.ts` | not supported | `node_modules` packages |
| `references/library-reference-7.ts` | not supported | `node_modules` packages |
| `references/library-reference-8.ts` | not supported | declaration files (`.d.ts`) |
| `references/library-reference-scoped-packages.ts` | not supported | `node_modules` packages |
| `salsa/annotatedThisPropertyInitializerDoesntNarrow.ts` | not supported | JavaScript |
| `salsa/assignmentToVoidZero1.ts` | not supported | JavaScript |
| `salsa/assignmentToVoidZero2.ts` | not supported | JavaScript |
| `salsa/binderUninitializedModuleExportsAssignment.ts` | not supported | JavaScript |
| `salsa/chainedPrototypeAssignment.ts` | not supported | JavaScript |
| `salsa/checkSpecialPropertyAssignments.ts` | not supported | JavaScript |
| `salsa/circularMultipleAssignmentDeclaration.ts` | not supported | JavaScript |
| `salsa/classCanExtendConstructorFunction.ts` | not supported | JavaScript |
| `salsa/commonJSAliasedExport.ts` | not supported | JavaScript |
| `salsa/commonJSImportClassTypeReference.ts` | not supported | JavaScript |
| `salsa/commonJSImportExportedClassExpression.ts` | not supported | JavaScript |
| `salsa/commonJSImportNestedClassTypeReference.ts` | not supported | JavaScript |
| `salsa/commonJSReexport.ts` | not supported | JavaScript |
| `salsa/conflictingCommonJSES2015Exports.ts` | not supported | JavaScript |
| `salsa/constructorFunctionMergeWithClass.ts` | not supported | JavaScript |
| `salsa/constructorFunctionMethodTypeParameters.ts` | not supported | JavaScript |
| `salsa/constructorFunctions.ts` | not supported | JavaScript |
| `salsa/constructorFunctions2.ts` | not supported | JavaScript |
| `salsa/constructorFunctions3.ts` | not supported | JavaScript |
| `salsa/constructorFunctionsStrict.ts` | not supported | JavaScript |
| `salsa/constructorNameInAccessor.ts` | not supported | parameter requires a type annotation, on ` set constructor(value) {} ` |
| `salsa/constructorNameInGenerator.ts` | not supported | expected class member name, on ` *constructor(): Generator<never, void, unknown> {} ` |
| `salsa/constructorNameInObjectLiteralAccessor.ts` | not supported | expected `,` or `}`, on ` get constructor() { return }, ` |
| `salsa/contextualTypedSpecialAssignment.ts` | not supported | JavaScript |
| `salsa/defaultPropertyAssignedClassWithPrototype.ts` | not supported | JavaScript |
| `salsa/enumMergeWithExpando.ts` | not supported | JavaScript |
| `salsa/expandoOnAlias.ts` | not supported | JavaScript |
| `salsa/exportDefaultInJsFile01.ts` | not supported | JavaScript |
| `salsa/exportDefaultInJsFile02.ts` | not supported | JavaScript |
| `salsa/exportNestedNamespaces.ts` | not supported | JavaScript |
| `salsa/exportNestedNamespaces2.ts` | not supported | JavaScript |
| `salsa/exportPropertyAssignmentNameResolution.ts` | not supported | JavaScript |
| `salsa/globalMergeWithCommonJSAssignmentDeclaration.ts` | not supported | JavaScript |
| `salsa/importAliasModuleExports.ts` | not supported | JavaScript |
| `salsa/importingExportingTypes.ts` | not supported | JavaScript |
| `salsa/inferingFromAny.ts` | not supported | JavaScript |
| `salsa/inferringClassMembersFromAssignments.ts` | not supported | JavaScript |
| `salsa/inferringClassMembersFromAssignments2.ts` | not supported | JavaScript |
| `salsa/inferringClassMembersFromAssignments3.ts` | not supported | JavaScript |
| `salsa/inferringClassMembersFromAssignments4.ts` | not supported | JavaScript |
| `salsa/inferringClassMembersFromAssignments5.ts` | not supported | JavaScript |
| `salsa/inferringClassMembersFromAssignments6.ts` | not supported | JavaScript |
| `salsa/inferringClassMembersFromAssignments7.ts` | not supported | JavaScript |
| `salsa/inferringClassMembersFromAssignments8.ts` | the port changes what it checks | `tsc` then reports TS2683, TS7009 |
| `salsa/inferringClassStaticMembersFromAssignments.ts` | not supported | JavaScript |
| `salsa/jsContainerMergeJsContainer.ts` | not supported | JavaScript |
| `salsa/jsContainerMergeTsDeclaration.ts` | not supported | JavaScript |
| `salsa/jsContainerMergeTsDeclaration2.ts` | not supported | JavaScript |
| `salsa/jsContainerMergeTsDeclaration3.ts` | not supported | JavaScript |
| `salsa/jsdocConstructorFunctionTypeReference.ts` | not supported | JavaScript |
| `salsa/jsObjectsMarkedAsOpenEnded.ts` | not supported | JavaScript |
| `salsa/lateBoundAssignmentDeclarationSupport1.ts` | not supported | JavaScript |
| `salsa/lateBoundAssignmentDeclarationSupport2.ts` | not supported | JavaScript |
| `salsa/lateBoundAssignmentDeclarationSupport3.ts` | not supported | JavaScript |
| `salsa/lateBoundAssignmentDeclarationSupport4.ts` | not supported | JavaScript |
| `salsa/lateBoundAssignmentDeclarationSupport5.ts` | not supported | JavaScript |
| `salsa/lateBoundAssignmentDeclarationSupport6.ts` | not supported | JavaScript |
| `salsa/lateBoundAssignmentDeclarationSupport7.ts` | not supported | JavaScript |
| `salsa/lateBoundClassMemberAssignmentJS.ts` | not supported | JavaScript |
| `salsa/lateBoundClassMemberAssignmentJS2.ts` | not supported | JavaScript |
| `salsa/lateBoundClassMemberAssignmentJS3.ts` | not supported | JavaScript |
| `salsa/malformedTags.ts` | not supported | JavaScript |
| `salsa/methodsReturningThis.ts` | not supported | JavaScript |
| `salsa/mixedPropertyElementAccessAssignmentDeclaration.ts` | the port changes what it checks | `tsc` then reports TS7034, TS7005 |
| `salsa/moduleExportAlias.ts` | not supported | JavaScript |
| `salsa/moduleExportAlias2.ts` | not supported | JavaScript |
| `salsa/moduleExportAlias3.ts` | not supported | JavaScript |
| `salsa/moduleExportAlias4.ts` | not supported | JavaScript |
| `salsa/moduleExportAlias5.ts` | not supported | JavaScript |
| `salsa/moduleExportAliasElementAccessExpression.ts` | not supported | JavaScript |
| `salsa/moduleExportAliasExports.ts` | not supported | JavaScript |
| `salsa/moduleExportAliasImported.ts` | not supported | JavaScript |
| `salsa/moduleExportAliasUnknown.ts` | not supported | JavaScript |
| `salsa/moduleExportAssignment.ts` | not supported | JavaScript |
| `salsa/moduleExportAssignment2.ts` | not supported | JavaScript |
| `salsa/moduleExportAssignment3.ts` | not supported | JavaScript |
| `salsa/moduleExportAssignment4.ts` | not supported | JavaScript |
| `salsa/moduleExportAssignment5.ts` | not supported | JavaScript |
| `salsa/moduleExportAssignment6.ts` | not supported | JavaScript |
| `salsa/moduleExportAssignment7.ts` | not supported | JavaScript |
| `salsa/moduleExportDuplicateAlias.ts` | not supported | JavaScript |
| `salsa/moduleExportDuplicateAlias2.ts` | not supported | JavaScript |
| `salsa/moduleExportDuplicateAlias3.ts` | not supported | JavaScript |
| `salsa/moduleExportNestedNamespaces.ts` | not supported | JavaScript |
| `salsa/moduleExportPropertyAssignmentDefault.ts` | not supported | JavaScript |
| `salsa/moduleExportsAliasLoop1.ts` | not supported | JavaScript |
| `salsa/moduleExportsAliasLoop2.ts` | not supported | JavaScript |
| `salsa/moduleExportWithExportPropertyAssignment.ts` | not supported | JavaScript |
| `salsa/moduleExportWithExportPropertyAssignment2.ts` | not supported | JavaScript |
| `salsa/moduleExportWithExportPropertyAssignment3.ts` | not supported | JavaScript |
| `salsa/moduleExportWithExportPropertyAssignment4.ts` | not supported | JavaScript |
| `salsa/multipleDeclarations.ts` | not supported | JavaScript |
| `salsa/namespaceAssignmentToRequireAlias.ts` | not supported | JavaScript |
| `salsa/nestedDestructuringOfRequire.ts` | not supported | JavaScript |
| `salsa/nestedPrototypeAssignment.ts` | not supported | JavaScript |
| `salsa/plainJSBinderErrors.ts` | not supported | JavaScript |
| `salsa/plainJSGrammarErrors.ts` | not supported | JavaScript |
| `salsa/plainJSGrammarErrors2.ts` | not supported | JavaScript |
| `salsa/plainJSGrammarErrors3.ts` | not supported | JavaScript |
| `salsa/plainJSGrammarErrors4.ts` | not supported | JavaScript |
| `salsa/plainJSRedeclare.ts` | not supported | JavaScript |
| `salsa/plainJSRedeclare2.ts` | not supported | JavaScript |
| `salsa/plainJSRedeclare3.ts` | not supported | JavaScript |
| `salsa/plainJSReservedStrict.ts` | not supported | JavaScript |
| `salsa/plainJSTypeErrors.ts` | not supported | JavaScript |
| `salsa/privateConstructorFunction.ts` | not supported | JavaScript |
| `salsa/privateIdentifierExpando.ts` | not supported | JavaScript |
| `salsa/propertiesOfGenericConstructorFunctions.ts` | not supported | JavaScript |
| `salsa/propertyAssignmentOnImportedSymbol.ts` | not supported | JavaScript |
| `salsa/propertyAssignmentOnParenthesizedNumber.ts` | not supported | JavaScript |
| `salsa/propertyAssignmentOnUnresolvedImportedSymbol.ts` | not supported | JavaScript |
| `salsa/propertyAssignmentUseParentType2.ts` | not supported | JavaScript |
| `salsa/propertyAssignmentUseParentType3.ts` | not supported | `any` is not supported, on ` function foo2(): any[] { ` |
| `salsa/prototypePropertyAssignmentMergeAcrossFiles.ts` | not supported | JavaScript |
| `salsa/prototypePropertyAssignmentMergeAcrossFiles2.ts` | not supported | JavaScript |
| `salsa/prototypePropertyAssignmentMergedTypeReference.ts` | not supported | JavaScript |
| `salsa/prototypePropertyAssignmentMergeWithInterfaceMethod.ts` | not supported | JavaScript |
| `salsa/reExportJsFromTs.ts` | not supported | JavaScript |
| `salsa/requireAssertsFromTypescript.ts` | not supported | JavaScript |
| `salsa/requireOfESWithPropertyAccess.ts` | not supported | JavaScript |
| `salsa/requireTwoPropertyAccesses.ts` | not supported | JavaScript |
| `salsa/sourceFileMergeWithFunction.ts` | not supported | declaration files (`.d.ts`) |
| `salsa/spellingUncheckedJS.ts` | not supported | JavaScript |
| `salsa/thisPropertyAssignment.ts` | not supported | JavaScript |
| `salsa/thisPropertyAssignmentCircular.ts` | not supported | JavaScript |
| `salsa/thisPropertyAssignmentComputed.ts` | not supported | JavaScript |
| `salsa/thisPropertyAssignmentInherited.ts` | not supported | JavaScript |
| `salsa/thisTypeOfConstructorFunctions.ts` | not supported | JavaScript |
| `salsa/topLevelThisAssignment.ts` | not supported | JavaScript |
| `salsa/typeFromContextualThisType.ts` | not supported | JavaScript |
| `salsa/typeFromJSConstructor.ts` | not supported | JavaScript |
| `salsa/typeFromJSInitializer.ts` | not supported | JavaScript |
| `salsa/typeFromJSInitializer2.ts` | not supported | JavaScript |
| `salsa/typeFromJSInitializer3.ts` | not supported | JavaScript |
| `salsa/typeFromJSInitializer4.ts` | not supported | JavaScript |
| `salsa/typeFromParamTagForFunction.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment10_1.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment10.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment11.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment12.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment13.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment14.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment15.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment16.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment17.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment18.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment19.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment2.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment20.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment21.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment22.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment23.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment24.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment25.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment26.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment27.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment28.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment3.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment32.ts` | not supported | script files sharing one global scope |
| `salsa/typeFromPropertyAssignment33.ts` | not supported | script files sharing one global scope |
| `salsa/typeFromPropertyAssignment34.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment35.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment37.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment39.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment4.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment40.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment5.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment6.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment7.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment8_1.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment8.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment9_1.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignment9.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignmentOutOfOrder.ts` | not supported | JavaScript |
| `salsa/typeFromPropertyAssignmentWithExport.ts` | not supported | JavaScript |
| `salsa/typeFromPrototypeAssignment.ts` | not supported | JavaScript |
| `salsa/typeFromPrototypeAssignment2.ts` | not supported | JavaScript |
| `salsa/typeFromPrototypeAssignment3.ts` | not supported | JavaScript |
| `salsa/typeFromPrototypeAssignment4.ts` | not supported | JavaScript |
| `salsa/typeLookupInIIFE.ts` | not supported | JavaScript |
| `salsa/typeTagOnFunctionReferencesGeneric.ts` | not supported | JavaScript |
| `salsa/unannotatedParametersAreOptional.ts` | not supported | JavaScript |
| `salsa/varRequireFromJavascript.ts` | not supported | JavaScript |
| `salsa/varRequireFromTypescript.ts` | not supported | JavaScript |
| `scanner/ecmascript3/scannerES3NumericLiteral1.ts` | checks too little | 0 after the port |
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
| `scanner/ecmascript5/scannerS7.2_A1.5_T2.ts` | duplicate | of `parser/ecmascript5/parserS7.2_A1.5_T2.ts` |
| `scanner/ecmascript5/scannerS7.3_A1.1_T2.ts` | not supported | expected identifier after `let`/`const`, on ` x ` |
| `scanner/ecmascript5/scannerS7.4_A2_T2.ts` | not supported | unterminated block comment, on ` /*CHECK#1/ ` |
| `scanner/ecmascript5/scannerS7.6_A4.2_T1.ts` | not supported | unexpected character `\`, on ` let \u0410 = 1; ` |
| `scanner/ecmascript5/scannerS7.8.3_A6.1_T1.ts` | not supported | missing digits after `0x`, on ` 0x ` |
| `scanner/ecmascript5/scannerS7.8.4_A7.1_T4.ts` | not supported | invalid unicode escape: expected 4 hex digits, on ` "\u000G" ` |
| `scanner/ecmascript5/scannerStringLiterals.ts` | not supported | unknown escape sequence `\ `, on ` '\u2192\   ' ` |
| `scanner/ecmascript5/scannerStringLiteralWithContainingNullCharacter1.ts` | checks too little | 0 after the port |
| `scanner/ecmascript5/scannerUnexpectedNullCharacter1.ts` | not supported | unexpected byte 0x00, on ` foo\u0000+\u0000bar; ` |
| `scanner/ecmascript5/scannerUnicodeEscapeInKeyword1.ts` | not supported | unexpected character `\`, on ` \u0076ar x = "hello"; ` |
| `scanner/ecmascript5/scannerUnicodeEscapeInKeyword2.ts` | porter failure | nothing to prune at offsets 75, 415, 84, 424, 84, 424; our first unsupported error: unexpected character `\` |
| `scanner/jsdocInvalidTokens.ts` | not supported | JavaScript |
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
| `statements/for-ofStatements/ES5For-of10.ts` | not supported | expected `;` after expression, on ` for (foo().x of []) { ` |
| `statements/for-ofStatements/ES5For-of11.ts` | not supported | `let` declaration requires an initializer, on ` let v; ` |
| `statements/for-ofStatements/ES5For-of12.ts` | not supported | expected `;` after expression, on ` for ([""] of [[""]]) { } ` |
| `statements/for-ofStatements/ES5For-of20.ts` | not supported | `let` declaration requires an initializer, on ` let v; ` |
| `statements/for-ofStatements/ES5For-of27.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (let {x: a = 0, y: b = 1} of [2, 3]) { ` |
| `statements/for-ofStatements/ES5For-of28.ts` | duplicate | of `statements/for-ofStatements/ES5For-of26.ts` |
| `statements/for-ofStatements/ES5For-of29.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (const {x: a = 0, y: b = 1} of [2, 3]) { ` |
| `statements/for-ofStatements/ES5For-of30.ts` | not supported | expected `;` after declaration, on ` let a: string = null as unknown as (string), b: number = null as unknown as (... ` |
| `statements/for-ofStatements/ES5For-of31.ts` | not supported | expected `;` after declaration, on ` let a: string = null as unknown as (string), b: number = null as unknown as (... ` |
| `statements/for-ofStatements/ES5For-of34.ts` | not supported | expected `;` after expression, on ` for (foo().x of ['a', 'b', 'c']) { ` |
| `statements/for-ofStatements/ES5For-of35.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (const {x: a = 0, y: b = 1} of [2, 3]) { ` |
| `statements/for-ofStatements/ES5For-of36.ts` | duplicate | of `statements/for-ofStatements/ES5For-of26.ts` |
| `statements/for-ofStatements/ES5For-of4.ts` | the port changes what it checks | `tsc` then reports TS1156, TS2304 |
| `statements/for-ofStatements/ES5For-of8.ts` | not supported | expected `;` after expression, on ` for (foo().x of ['a', 'b', 'c']) { ` |
| `statements/for-ofStatements/ES5For-of9.ts` | not supported | expected `;` after expression, on ` for (foo().x of []) { ` |
| `statements/for-ofStatements/ES5For-ofTypeCheck10.ts` | not supported | expected class member name, on ` [Symbol.iterator](): this { ` |
| `statements/for-ofStatements/ES5For-ofTypeCheck11.ts` | not supported | expected `;` after expression, on ` for (v of union) { } ` |
| `statements/for-ofStatements/ES5For-ofTypeCheck14.ts` | not supported | `as` to `string \| Set<number>` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let union: string \| Set<number> = null as unknown as (string \| Set<number>); ` |
| `statements/for-ofStatements/ES5For-ofTypeCheck8.ts` | not supported | expected `;` after expression, on ` for (v of union) { } ` |
| `statements/for-ofStatements/ES5For-ofTypeCheck9.ts` | not supported | unknown type `symbol`, on ` let union: string \| string[] \| number \| symbol = null as unknown as (string \|... ` |
| `statements/forStatements/forStatementsMultipleInvalidDecl.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `statements/labeledStatements/` | not supported | labeled statements |
| `statements/returnStatements/returnStatementNoAsiAfterTransform.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `statements/switchStatements/switchStatements.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `statements/throwStatements/invalidThrowStatement.ts` | not supported | expected expression after `throw`, on ` throw; ` |
| `statements/tryStatements/catchClauseWithTypeAnnotation.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `statements/tryStatements/invalidTryStatements.ts` | not supported | expected expression, on ` catch(x) { } // error missing try ` |
| `statements/tryStatements/tryStatements.ts` | not supported | `any` is not supported, on ` try { } catch (x) { let x: any = null as unknown as (any); } ` |
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
| `types/contextualTypes/methodDeclarations/contextuallyTypedClassExpressionMethodDeclaration01.ts` | not supported | expected expression, on ` return class { ` |
| `types/contextualTypes/methodDeclarations/contextuallyTypedClassExpressionMethodDeclaration02.ts` | not supported | construct signatures |
| `types/contextualTypes/partiallyAnnotatedFunction/partiallyAnnotatedFunctionInferenceError.ts` | not supported | expected `,` or `>`, on ` function testError<T extends C>(a: (t: T, t1: T) => void): T { return null as... ` |
| `types/contextualTypes/partiallyAnnotatedFunction/partiallyAnnotatedFunctionInferenceWithTypeParameter.ts` | not supported | expected `,` or `>`, on ` function test<T extends C>(a: (t: T, t1: T) => void): T { return null as unkn... ` |
| `types/forAwait/` | not supported | async/await |
| `types/import/` | not supported | `import()` types |
| `types/intersection/` | not supported | intersection types |
| `types/keyof/circularIndexedAccessErrors.ts` | not supported | expected `]` to close array type, on ` x: T1["x"];  // Error ` |
| `types/keyof/keyofAndForIn.ts` | not supported | expected `,` or `>`, on ` function f1<K extends string, T>(obj: { [P in K]: T }, k: K): void { ` |
| `types/keyof/keyofAndIndexedAccess.ts` | not supported | index signatures require string keys; use `[key: string]: V`, on ` type NumericallyIndexed<T> = { [x: number]: T }; ` |
| `types/keyof/keyofAndIndexedAccess2.ts` | not supported | expected `,` or `>`, on ` function f2<T extends { [key: string]: number }>(a: { x: number, y: number },... ` |
| `types/keyof/keyofAndIndexedAccessErrors.ts` | not supported | expected `]` to close array type, on ` type T10 = Shape["name"]; ` |
| `types/keyof/keyofIntersection.ts` | not supported | intersection types |
| `types/literal/booleanLiteralTypes2.ts` | duplicate | of `types/literal/booleanLiteralTypes1.ts` |
| `types/literal/enumLiteralTypes1.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum Choice { Unknown, Yes, No }; ` |
| `types/literal/enumLiteralTypes2.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum Choice { Unknown, Yes, No }; ` |
| `types/literal/enumLiteralTypes3.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum Choice { Unknown, Yes, No }; ` |
| `types/literal/numericLiteralTypes2.ts` | duplicate | of `types/literal/numericLiteralTypes1.ts` |
| `types/literal/numericStringLiteralTypes.ts` | not supported | expected `;` after type alias body, on `` type T0 = string & `${string}`;  // string `` |
| `types/literal/stringEnumLiteralTypes1.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum Choice { Unknown = "", Yes = "yes", No = "no" }; ` |
| `types/literal/stringEnumLiteralTypes2.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum Choice { Unknown = "", Yes = "yes", No = "no" }; ` |
| `types/literal/stringEnumLiteralTypes3.ts` | not supported | `enum` is a reserved keyword and can't be used as a name, on ` const enum Choice { Unknown = "", Yes = "yes", No = "no" }; ` |
| `types/literal/stringLiteralsAssertionsInEqualityComparisons01.ts` | not supported | `any` is not supported, on ` let c = "foo" == (<any>"bar"); ` |
| `types/literal/stringLiteralsAssertionsInEqualityComparisons02.ts` | not supported | expected `;` after type alias body, on ` type EnhancedString = string & { enhancements: any }; ` |
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
| `types/localTypes/localTypes1.ts` | not supported | a `class` must be declared at the top level of the module, on ` class C { ` |
| `types/localTypes/localTypes2.ts` | not supported | a `class` must be declared at the top level of the module, on ` class C { ` |
| `types/localTypes/localTypes3.ts` | not supported | a `class` must be declared at the top level of the module, on ` class C<X, Y> { ` |
| `types/localTypes/localTypes4.ts` | not supported | unknown type `T`, on ` let v: T = null as unknown as (T); ` |
| `types/localTypes/localTypes5.ts` | not supported | expected `{`, on ` function foo<A>(): X.m<number, boolean>.(Anonymous function)<Date>.Y<string> { ` |
| `types/mapped/` | not supported | mapped types |
| `types/members/augmentedTypeAssignmentCompatIndexSignature.ts` | not supported | index signatures |
| `types/members/augmentedTypeBracketAccessIndexSignature.ts` | not supported | index signatures |
| `types/members/classWithPrivateProperty.ts` | not supported | expected `:` and a type for the class field, on ` private x; ` |
| `types/members/classWithProtectedProperty.ts` | not supported | `protected` is not supported, on ` protected x; ` |
| `types/members/classWithPublicProperty.ts` | not supported | expected `:` and a type for the class field, on ` public x; ` |
| `types/members/duplicateNumericIndexers.ts` | not supported | index signatures |
| `types/members/duplicateStringIndexers.ts` | not supported | index signatures |
| `types/members/indexSignatures1.ts` | not supported | index signatures require string keys; use `[key: string]: V`, on ` function gg3(x: { [key: string]: string }, y: { [key: symbol]: string }, z: {... ` |
| `types/members/objectTypeHidingMembersOfExtendedObject.ts` | not supported | expected `:` and a type for the class field, on ` foo!: string; ` |
| `types/members/objectTypeHidingMembersOfObjectAssignmentCompat.ts` | not supported | `as` to `I` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let i: I = null as unknown as (I); ` |
| `types/members/objectTypeHidingMembersOfObjectAssignmentCompat2.ts` | not supported | `as` to `I` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let i: I = null as unknown as (I); ` |
| `types/members/objectTypeWithCallSignatureAppearsToBeFunctionType.ts` | not supported | `any` is not supported, on ` let r2b: (x: any, y?: any) => any = i.apply; ` |
| `types/members/objectTypeWithCallSignatureHidingMembersOfExtendedFunction.ts` | not supported | `any` is not supported, on ` apply(a: any, b?: any): void; ` |
| `types/members/objectTypeWithCallSignatureHidingMembersOfFunction.ts` | not supported | `any` is not supported, on ` apply(a: any, b?: any): void; ` |
| `types/members/objectTypeWithCallSignatureHidingMembersOfFunctionAssignmentCompat.ts` | not supported | expected field name in object type, on ` (): void ` |
| `types/members/objectTypeWithConstructSignatureAppearsToBeFunctionType.ts` | not supported | construct signatures |
| `types/members/objectTypeWithConstructSignatureHidingMembersOfExtendedFunction.ts` | not supported | construct signatures |
| `types/members/objectTypeWithConstructSignatureHidingMembersOfFunction.ts` | not supported | construct signatures |
| `types/members/objectTypeWithConstructSignatureHidingMembersOfFunctionAssignmentCompat.ts` | not supported | construct signatures |
| `types/members/objectTypeWithDuplicateNumericProperty.ts` | not supported | expected class member name, on ` 1; ` |
| `types/members/objectTypeWithNumericProperty.ts` | not supported | expected class member name, on ` 1: number; ` |
| `types/members/objectTypeWithStringAndNumberIndexSignatureToAny.ts` | not supported | index signatures |
| `types/members/objectTypeWithStringIndexerHidingObjectIndexer.ts` | not supported | index signatures |
| `types/members/objectTypeWithStringNamedNumericProperty.ts` | not supported | `Date` is not supported, on ` "1.0": Date; ` |
| `types/members/typesWithPrivateConstructor.ts` | not supported | expected `{`, on ` private constructor(x: number); ` |
| `types/members/typesWithProtectedConstructor.ts` | not supported | `protected` is not supported, on ` protected constructor() { } ` |
| `types/members/typesWithPublicConstructor.ts` | not supported | expected `{`, on ` public constructor(x: number); ` |
| `types/members/typesWithSpecializedCallSignatures.ts` | not supported | expected `{`, on ` foo(x: 'hi'): Derived1; ` |
| `types/members/typesWithSpecializedConstructSignatures.ts` | not supported | construct signatures |
| `types/namedTypes/classWithOnlyPublicMembersEquivalentToInterface.ts` | not supported | parameter requires a type annotation, on ` public set z(v) { } ` |
| `types/namedTypes/classWithOnlyPublicMembersEquivalentToInterface2.ts` | not supported | parameter requires a type annotation, on ` public set z(v) { } ` |
| `types/namedTypes/classWithOptionalParameter.ts` | checks too little | 0 after the port |
| `types/namedTypes/interfaceWithPrivateMember.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` private x: string; ` |
| `types/never/neverIntersectionNotCallable.ts` | not supported | intersection types |
| `types/never/neverTypeErrors2.ts` | duplicate | of `types/never/neverTypeErrors1.ts` |
| `types/never/neverUnionIntersection.ts` | not supported | intersection types |
| `types/nonPrimitive/nonPrimitiveAndEmptyObject.ts` | not supported | expected `;`, `,`, or `}` after interface member, on ` fooProps?: BarProps & object; ` |
| `types/nonPrimitive/nonPrimitiveAndTypeVariables.ts` | not supported | expected `:` after index parameter name, on ` type A<T, V> = { [P in keyof T]: T[P] extends V ? 1 : 0; }; ` |
| `types/nonPrimitive/nonPrimitiveConstraintOfIndexAccessType.ts` | not supported | expected `,` or `>`, on ` function f<T extends object, P extends keyof T>(s: string, tp: T[P]): void { ` |
| `types/nonPrimitive/nonPrimitiveIndexingWithForIn.ts` | not supported | `let` declaration requires an initializer, on ` for (let key in a) { ` |
| `types/nonPrimitive/nonPrimitiveIndexingWithForInNoImplicitAny.ts` | not supported | `let` declaration requires an initializer, on ` for (let key in a) { ` |
| `types/nonPrimitive/nonPrimitiveIndexingWithForInSupressError.ts` | not supported | `let` declaration requires an initializer, on ` for (let key in a) { ` |
| `types/nonPrimitive/nonPrimitiveUnionIntersection.ts` | not supported | intersection types |
| `types/objectTypeLiteral/callSignatures/callSignaturesThatDifferOnlyByReturnType.ts` | not supported | parameter requires a type annotation, on ` (x): number; ` |
| `types/objectTypeLiteral/callSignatures/callSignaturesThatDifferOnlyByReturnType2.ts` | not supported | `as` to `A` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let x: A = null as unknown as (A); ` |
| `types/objectTypeLiteral/callSignatures/callSignaturesThatDifferOnlyByReturnType3.ts` | not supported | duplicate declaration of interface `I`, on ` interface I { ` |
| `types/objectTypeLiteral/callSignatures/callSignaturesWithAccessibilityModifiersOnParameters.ts` | not supported | parameter requires a type annotation, on ` function foo(public x, private y): void { } ` |
| `types/objectTypeLiteral/callSignatures/callSignaturesWithDuplicateParameters.ts` | not supported | parameter requires a type annotation, on ` function foo(x, x): void { } ` |
| `types/objectTypeLiteral/callSignatures/callSignaturesWithOptionalParameters2.ts` | not supported | expected `:` and return type, on ` function foo(x?: number); ` |
| `types/objectTypeLiteral/callSignatures/callSignaturesWithParameterInitializers2.ts` | not supported | expected `:` and return type, on ` function foo(x: number = 2); ` |
| `types/objectTypeLiteral/callSignatures/callSignatureWithOptionalParameterAndInitializer.ts` | not supported | expected `:` and return type, on ` (x?: number = 1); ` |
| `types/objectTypeLiteral/callSignatures/callSignatureWithoutAnnotationsOrBody.ts` | not supported | parameter requires a type annotation, on ` function foo(x): void { } ` |
| `types/objectTypeLiteral/callSignatures/constructSignatureWithAccessibilityModifiersOnParameters.ts` | not supported | construct signatures |
| `types/objectTypeLiteral/callSignatures/constructSignatureWithAccessibilityModifiersOnParameters2.ts` | not supported | construct signatures |
| `types/objectTypeLiteral/callSignatures/identicalCallSignatures.ts` | not supported | parameter requires a type annotation, on ` (x): number; ` |
| `types/objectTypeLiteral/callSignatures/identicalCallSignatures2.ts` | checks too little | 0 after the port |
| `types/objectTypeLiteral/callSignatures/identicalCallSignatures3.ts` | not supported | duplicate declaration of interface `I`, on ` interface I { ` |
| `types/objectTypeLiteral/callSignatures/parametersWithNoAnnotationAreAny.ts` | not supported | parameter requires a type annotation, on ` function foo(x): any { return x; } ` |
| `types/objectTypeLiteral/callSignatures/restParametersOfNonArrayTypes.ts` | not supported | expected `:` and return type, on ` (...x: string); ` |
| `types/objectTypeLiteral/callSignatures/restParametersOfNonArrayTypes2.ts` | not supported | `any` is not supported, on ` interface MyThing extends Array<any> { } ` |
| `types/objectTypeLiteral/callSignatures/restParametersWithArrayTypeAnnotations.ts` | not supported | expected `:` and return type, on ` (...x: number[]); ` |
| `types/objectTypeLiteral/callSignatures/restParameterWithoutAnnotationIsAnyArray.ts` | not supported | rest parameter requires a type annotation, on ` function foo(...x): void { } ` |
| `types/objectTypeLiteral/callSignatures/specializedSignatureIsNotSubtypeOfNonSpecializedSignature.ts` | not supported | expected `:` and return type, on ` function foo(x: 'a'); ` |
| `types/objectTypeLiteral/callSignatures/specializedSignatureIsSubtypeOfNonSpecializedSignature.ts` | not supported | expected `:` and return type, on ` function foo(x: 'a'); ` |
| `types/objectTypeLiteral/callSignatures/specializedSignatureWithOptional.ts` | the port changes what it checks | `tsc` then reports TS2393 |
| `types/objectTypeLiteral/callSignatures/stringLiteralTypesInImplementationSignatures2.ts` | not supported | `any` is not supported, on ` function foo(x: any); ` |
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
| `types/objectTypeLiteral/methodSignatures/objectTypesWithOptionalProperties2.ts` | not supported | expected `:` and return type after method parameters, on ` x()?: number; // error ` |
| `types/objectTypeLiteral/propertySignatures/numericNamedPropertyDuplicates.ts` | not supported | expected class member name, on ` 1: number; ` |
| `types/objectTypeLiteral/propertySignatures/numericStringNamedPropertyEquivalence.ts` | not supported | expected class member name, on ` 1.0: number; ` |
| `types/objectTypeLiteral/propertySignatures/propertyNamesOfReservedWords.ts` | not supported | abstract classes are not supported, on ` abstract; ` |
| `types/objectTypeLiteral/propertySignatures/propertyNameWithoutTypeAnnotation.ts` | not supported | expected `:` and a type for the class field, on ` foo; ` |
| `types/primitives/boolean/assignFromBooleanInterface.ts` | not supported | `as` to `Boolean` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let a: Boolean = null as unknown as (Boolean); ` |
| `types/primitives/boolean/assignFromBooleanInterface2.ts` | not supported | duplicate declaration of interface `Boolean`, on ` interface Boolean { ` |
| `types/primitives/boolean/extendBooleanInterface.ts` | not supported | adding to a built-in interface |
| `types/primitives/boolean/validBooleanAssignments.ts` | not supported | `any` is not supported, on ` let a: any = x; ` |
| `types/primitives/number/assignFromNumberInterface.ts` | not supported | `as` to `Number` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let a: Number = null as unknown as (Number); ` |
| `types/primitives/number/assignFromNumberInterface2.ts` | not supported | duplicate declaration of interface `Number`, on ` interface Number { ` |
| `types/primitives/number/extendNumberInterface.ts` | not supported | adding to a built-in interface |
| `types/primitives/string/assignFromStringInterface.ts` | not supported | `as` to `String` is not yet supported: interfaces with methods are nominal — a plain structural check can't verify their vtable at runtime, on ` let a: String = null as unknown as (String); ` |
| `types/primitives/string/assignFromStringInterface2.ts` | not supported | `any` is not supported, on ` replace(searchValue: string, replaceValue: (substring: string, ...args: any[]... ` |
| `types/primitives/string/extendStringInterface.ts` | not supported | adding to a built-in interface |
| `types/primitives/string/validStringAssignments.ts` | not supported | `any` is not supported, on ` let a: any = x; ` |
| `types/primitives/stringLiteral/stringLiteralType.ts` | not supported | expected `:` and return type, on ` function f(x: 'hi'); ` |
| `types/primitives/void/validVoidAssignments.ts` | not supported | `any` is not supported, on ` let y: any = null as unknown as (any); ` |
| `types/rest/genericObjectRest.ts` | not supported | expected `,` or `>`, on ` function f1<T extends { a: string, b: number }>(obj: T): void { ` |
| `types/rest/genericRestArity.ts` | not supported | expected `,` or `>`, on ` function call<TS extends unknown[]>( ` |
| `types/rest/genericRestArityStrict.ts` | not supported | expected `,` or `>`, on ` function call<TS extends unknown[]>( ` |
| `types/rest/genericRestParameters1.ts` | not supported | rest parameter type must be an array, on ` let f1: (...x: [number, string, boolean]) => void = null as unknown as ((...x... ` |
| `types/rest/genericRestParameters2.ts` | not supported | rest elements in tuple types are not supported, on ` const t1: [number, string, ...boolean[]] = null as unknown as ([number, strin... ` |
| `types/rest/genericRestParameters3.ts` | not supported | rest parameter type must be an array, on ` let f1: (x: string, ...args: [string] \| [number, boolean]) => void = null as ... ` |
| `types/rest/objectRest.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `types/rest/objectRest2.ts` | not supported | `any` is not supported, on ` function connectionFromArray(objects: number, args: any): {} { return null as... ` |
| `types/rest/objectRestAssignment.ts` | not supported | `any` is not supported, on ` let ka: any = null as unknown as (any); ` |
| `types/rest/objectRestCatchES5.ts` | not supported | expected `;` after declaration, on ` let a = 1, b = 2; ` |
| `types/rest/objectRestForOf.ts` | not supported | object destructuring is not supported in `for-of`; use array destructuring or unpack inside the loop body, on ` for (let { x, ...restOf } of array) { ` |
| `types/rest/objectRestNegative.ts` | not supported | expected expression, on ` let { ...mustBeLast, a } = o; ` |
| `types/rest/objectRestParameter.ts` | not supported | expected `:` after field name, on ` function suddenly(f: (a: { x: { z, ka }, y: string }) => void): void { } ` |
| `types/rest/objectRestParameterES5.ts` | not supported | expected `:` after field name, on ` function suddenly(f: (a: { x: { z, ka }, y: string }) => void): void { } ` |
| `types/rest/objectRestPropertyMustBeLast.ts` | not supported | expected expression, on ` let {...a, x } = { x: 1 };    // Error, rest must be last property ` |
| `types/rest/objectRestReadonly.ts` | not supported | unknown type `Readonly`, on ` const obj: Readonly<ObjType> = { ` |
| `types/rest/restTuplesFromContextualTypes.ts` | not supported | expected expression, on ` (function (a, b, c){})(...t1); ` |
| `types/specifyingTypes/predefinedTypes/objectTypesWithPredefinedTypesAsName2.ts` | not supported | `void` is a reserved keyword and can't be used as a name, on ` class void {} // parse error unlike the others ` |
| `types/specifyingTypes/typeLiterals/arrayOfFunctionTypes3.ts` | not supported | expected `(` after constructor name in `new` expression, on ` let r3 = new y[0](); ` |
| `types/specifyingTypes/typeLiterals/arrayTypeOfFunctionTypes.ts` | not supported | expected field name in object type, on ` let x2: { (): string }[] = null as unknown as ({ (): string }[]); ` |
| `types/specifyingTypes/typeLiterals/arrayTypeOfFunctionTypes2.ts` | not supported | construct signatures |
| `types/specifyingTypes/typeLiterals/arrayTypeOfTypeOf.ts` | not supported | `let` declaration requires an initializer, on ` let xs3: typeof Array<number> = null as unknown as (typeof Array<number>); ` |
| `types/specifyingTypes/typeLiterals/functionLiteral.ts` | not supported | construct signatures |
| `types/specifyingTypes/typeLiterals/functionLiteralForOverloads.ts` | not supported | expected field name in object type, on ` (x: string): string; ` |
| `types/specifyingTypes/typeLiterals/functionLiteralForOverloads2.ts` | not supported | construct signatures |
| `types/specifyingTypes/typeLiterals/parenthesizedTypes.ts` | not supported | expected field name in object type, on ` let d_2: ({ (x: string): string } \| { (x: number): number })[] = null as unkn... ` |
| `types/specifyingTypes/typeLiterals/unionTypeLiterals.ts` | not supported | construct signatures |
| `types/specifyingTypes/typeQueries/invalidTypeOfTarget.ts` | not supported | expected a value name after `typeof`, on ` let x1: typeof = null as unknown as (typeof) {}; ` |
| `types/specifyingTypes/typeQueries/typeofClass2.ts` | not supported | expected `{`, on ` constructor(x: number); ` |
| `types/specifyingTypes/typeQueries/typeofClassWithPrivates.ts` | not supported | `as` to `C<string>` is not yet supported: class types aren't yet supported as `as` targets, on ` let c: C<string> = null as unknown as (C<string>); ` |
| `types/specifyingTypes/typeQueries/typeofModuleWithoutExports.ts` | not supported | expected `;` after expression, on ` namespace M { ` |
| `types/specifyingTypes/typeQueries/typeofThisWithImplicitThis.ts` | not supported | `this` is a reserved keyword and can't be used as a name, on ` let x: typeof this.no = 1 ` |
| `types/specifyingTypes/typeQueries/typeofTypeParameter.ts` | not supported | `let` declaration requires an initializer, on ` let a!: typeof x = null as unknown as (typeof x); ` |
| `types/specifyingTypes/typeQueries/typeQueryOnClass.ts` | not supported | expected `{`, on ` constructor(x: number); ` |
| `types/specifyingTypes/typeQueries/typeQueryWithReservedWords.ts` | not supported | `Controller` is a class, not a value, so `typeof` has no type to take, on ` create: typeof Controller.prototype.create; ` |
| `types/specifyingTypes/typeReferences/genericTypeReferenceWithoutTypeArgument3.ts` | not supported | expected `;` after expression, on ` declare class C<T> { ` |
| `types/spread/objectSpreadComputedProperty.ts` | not supported | `any` is not supported, on ` let a: any = null; ` |
| `types/spread/objectSpreadNegativeParse.ts` | not supported | expected expression, on ` let o7 = { ...o? }; ` |
| `types/spread/objectSpreadNoTransform.ts` | not supported | `let` declaration requires an initializer, on ` let b; ` |
| `types/spread/objectSpreadSetonlyAccessor.ts` | not supported | expected `,` or `}`, on ` const o1: { foo: number, bar: undefined } = { foo: 1, ... { set bar(_v: numbe... ` |
| `types/spread/spreadNonObject1.ts` | not supported | expected type, on `` type S = `${number}`; `` |
| `types/spread/spreadObjectOrFalsy.ts` | not supported | expected `,` or `)`, on ` function f1<T>(a: T & undefined): any { ` |
| `types/spread/spreadTypeVariable.ts` | not supported | expected `,` or `>`, on ` function f1<T extends number>(arg: T): any { ` |
| `types/stringLiteral/stringLiteralTypesAsTypeParameterConstraint01.ts` | not supported | expected `,` or `>`, on ` function foo<T extends "foo">(f: (x: T) => T): (x: T) => T { ` |
| `types/stringLiteral/stringLiteralTypesAsTypeParameterConstraint02.ts` | not supported | expected `,` or `>`, on ` function foo<T extends "foo">(f: (x: T) => T): (x: T) => T { ` |
| `types/stringLiteral/stringLiteralTypesInUnionTypes01.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/stringLiteral/stringLiteralTypesOverloadAssignability03.ts` | duplicate | of `types/stringLiteral/stringLiteralTypesOverloadAssignability01.ts` |
| `types/stringLiteral/stringLiteralTypesOverloadAssignability05.ts` | duplicate | of `types/stringLiteral/stringLiteralTypesOverloadAssignability01.ts` |
| `types/stringLiteral/stringLiteralTypesOverloads05.ts` | not supported | expected `{`, on ` function doThing(x: "dog"): Dog; ` |
| `types/stringLiteral/stringLiteralTypesTypePredicates01.ts` | not supported | expected `{`, on ` function kindIs(kind: Kind, is: "A"): kind is "A"; ` |
| `types/stringLiteral/typeArgumentsWithStringLiteralTypes01.ts` | not supported | expected `;` after expression, on ` namespace n1 { ` |
| `types/thisType/` | not supported | `this` types |
| `types/tuple/arityAndOrderCompatibility01.ts` | not supported | expected interface member name, on ` 0: string; ` |
| `types/tuple/castingTuple.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `types/tuple/contextualTypeTupleEnd.ts` | not supported | rest elements in tuple types are not supported, on ` type Funcs = [...((arg: number) => void)[], (arg: string) => void]; ` |
| `types/tuple/emptyTuples/emptyTuplesTypeAssertion01.ts` | not supported | tuple types must have at least one element, on ` let x = <[]>[]; ` |
| `types/tuple/emptyTuples/emptyTuplesTypeAssertion02.ts` | not supported | tuple types must have at least one element, on ` let x = [] as []; ` |
| `types/tuple/named/namedTupleMembersErrors.ts` | not supported | `any` is not supported, on ` export type List = [item: any, ...any]; ` |
| `types/tuple/named/partiallyNamedTuples.ts` | not supported | expected `]` to close array type, on ` function fb3(a: NamedAnonymousMixed, ...args: NamedAnonymousMixed[3]): void {} ` |
| `types/tuple/named/partiallyNamedTuples2.ts` | not supported | expected `,` or `>`, on ` interface MultiKeyMap<Keys extends readonly unknown[], Value> { ` |
| `types/tuple/named/partiallyNamedTuples3.ts` | not supported | expected expression, on ` const output = ((...args) => args)(...tuple); ` |
| `types/tuple/restTupleElements1.ts` | not supported | a required tuple element cannot follow an optional element, on ` type T02 = [string?, string];  // Error ` |
| `types/tuple/strictTupleLength.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `types/tuple/tupleElementTypes1.ts` | not supported | `any` is not supported, on ` let [a, b]: [number, any] = [undefined, undefined]; ` |
| `types/tuple/tupleElementTypes2.ts` | not supported | `any` is not supported, on ` function f([a, b]: [number, any]): void { } ` |
| `types/tuple/tupleLengthCheck.ts` | not supported | rest elements in tuple types are not supported, on ` const rest: [number, string, ...boolean[]] = null as unknown as ([number, str... ` |
| `types/tuple/typeInferenceWithTupleType.ts` | not supported | expected expression, on ` for (let i = 0; i < length; ++i) { ` |
| `types/tuple/unionsOfTupleTypes1.ts` | not supported | rest elements in tuple types are not supported, on ` type T3 = [string, ...number[]]; ` |
| `types/tuple/variadicTuples1.ts` | not supported | expected `,` or `>`, on ` type TV0<T extends unknown[]> = [string, ...T]; ` |
| `types/tuple/variadicTuples2.ts` | not supported | rest elements in tuple types are not supported, on ` type V00 = [number, ...string[]]; ` |
| `types/tuple/variadicTuples3.ts` | not supported | expected `,` or `>`, on ` function test1<T extends any[], P extends any[]>(): [...T, ...P] { ` |
| `types/tuple/wideningTuples1.ts` | not supported | expected `,` or `>`, on ` function foo<T extends [any]>(x: T): T { return null as unknown as (T); } ` |
| `types/tuple/wideningTuples2.ts` | not supported | `any` is not supported, on ` let foo: () => [any] = function bar() { ` |
| `types/tuple/wideningTuples3.ts` | not supported | `any` is not supported, on ` let a: [any] = null as unknown as ([any]); ` |
| `types/tuple/wideningTuples4.ts` | not supported | `any` is not supported, on ` let a: [any] = null as unknown as ([any]); ` |
| `types/tuple/wideningTuples6.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/typeAliases/asiPreventsParsingAsTypeAlias01.ts` | not supported | `let` declaration requires an initializer, on ` let type; ` |
| `types/typeAliases/asiPreventsParsingAsTypeAlias02.ts` | not supported | `let` declaration requires an initializer, on ` let type; ` |
| `types/typeAliases/circularTypeAliasForUnionWithClass.ts` | not supported | expected class member name, on ` [x: number]: T3; ` |
| `types/typeAliases/circularTypeAliasForUnionWithInterface.ts` | not supported | construct signatures |
| `types/typeAliases/classDoesNotDependOnBaseTypes.ts` | not supported | expected class member name, on ` [n: number]: StringTree; ` |
| `types/typeAliases/directDependenceBetweenTypeAliases.ts` | not supported | construct signatures |
| `types/typeAliases/intrinsicKeyword.ts` | not supported | string mapping types |
| `types/typeAliases/intrinsicTypes.ts` | not supported | string mapping types |
| `types/typeAliases/typeAliases.ts` | not supported | construct signatures |
| `types/typeParameters/recurringTypeParamForContainerOfBase01.ts` | not supported | expected `,` or `>`, on ` interface BoxOfFoo<T extends Foo<T>> { ` |
| `types/typeParameters/typeArgumentLists/constraintSatisfactionWithAny.ts` | not supported | expected `,` or `>`, on ` function foo<T extends String>(x: T): T { return null; } ` |
| `types/typeParameters/typeArgumentLists/constraintSatisfactionWithAny2.ts` | not supported | expected `,` or `>`, on ` function foo<Z, T extends <U>(x: U) => Z>(y: T): Z { return null as unknown a... ` |
| `types/typeParameters/typeArgumentLists/constraintSatisfactionWithEmptyObject.ts` | not supported | expected `,` or `>`, on ` function foo<T extends Object>(x: T): void { } ` |
| `types/typeParameters/typeArgumentLists/functionConstraintSatisfaction.ts` | not supported | construct signatures |
| `types/typeParameters/typeArgumentLists/functionConstraintSatisfaction2.ts` | not supported | construct signatures |
| `types/typeParameters/typeArgumentLists/functionConstraintSatisfaction3.ts` | not supported | construct signatures |
| `types/typeParameters/typeArgumentLists/instantiateNonGenericTypeWithTypeArguments.ts` | not supported | expected `:` and a type for the class field, on ` x!: string; ` |
| `types/typeParameters/typeArgumentLists/instantiationExpressionErrors.ts` | not supported | expected field name in object type, on ` let f: { <T>(): T, g<U>(): U } = null as unknown as ({ <T>(): T, g<U>(): U }); ` |
| `types/typeParameters/typeArgumentLists/instantiationExpressions.ts` | not supported | construct signatures |
| `types/typeParameters/typeArgumentLists/typeParameterAsTypeParameterConstraint.ts` | not supported | expected `,` or `>`, on ` function foo<T, U extends T>(x: T, y: U): U { return y; } ` |
| `types/typeParameters/typeArgumentLists/typeParameterAsTypeParameterConstraint2.ts` | not supported | expected `,` or `>`, on ` function foo<T, U extends T>(x: T, y: U): U { return y; } // this is now an e... ` |
| `types/typeParameters/typeArgumentLists/wrappedAndRecursiveConstraints.ts` | not supported | expected `,` or `>`, on ` class C<T extends Date> { ` |
| `types/typeParameters/typeArgumentLists/wrappedAndRecursiveConstraints2.ts` | not supported | expected `,` or `>`, on ` class C<T extends C<T>> { // error ` |
| `types/typeParameters/typeArgumentLists/wrappedAndRecursiveConstraints3.ts` | not supported | expected `,` or `>`, on ` class C<T extends { length: number }> { ` |
| `types/typeParameters/typeArgumentLists/wrappedAndRecursiveConstraints4.ts` | not supported | expected `,` or `>`, on ` class C<T extends { length: number }> { ` |
| `types/typeParameters/typeParameterLists/innerTypeParameterShadowingOuterOne.ts` | not supported | expected `,` or `>`, on ` function f<T extends Date>(): void { ` |
| `types/typeParameters/typeParameterLists/innerTypeParameterShadowingOuterOne2.ts` | not supported | expected `,` or `>`, on ` class C<T extends Date> { ` |
| `types/typeParameters/typeParameterLists/propertyAccessOnTypeParameterWithConstraints.ts` | not supported | expected `,` or `>`, on ` class C<T extends Date> { ` |
| `types/typeParameters/typeParameterLists/propertyAccessOnTypeParameterWithConstraints2.ts` | not supported | expected `,` or `>`, on ` class C<U extends A, T extends A> { ` |
| `types/typeParameters/typeParameterLists/propertyAccessOnTypeParameterWithConstraints3.ts` | not supported | expected `,` or `>`, on ` class C<U extends A, T extends U> { ` |
| `types/typeParameters/typeParameterLists/propertyAccessOnTypeParameterWithConstraints4.ts` | not supported | expected `,` or `>`, on ` class C<T extends Date> { ` |
| `types/typeParameters/typeParameterLists/propertyAccessOnTypeParameterWithConstraints5.ts` | not supported | expected `,` or `>`, on ` class C<U extends T, T extends A> { ` |
| `types/typeParameters/typeParameterLists/propertyAccessOnTypeParameterWithoutConstraints.ts` | not supported | `any` is not supported, on ` f(): any { ` |
| `types/typeParameters/typeParameterLists/staticMembersUsingClassTypeParameter.ts` | not supported | expected `,` or `>`, on ` class C3<T extends Date> { ` |
| `types/typeParameters/typeParameterLists/typeParameterConstModifiers.ts` | not supported | construct signatures |
| `types/typeParameters/typeParameterLists/typeParameterConstModifiersReturnsAndYields.ts` | not supported | `const` is a reserved keyword and can't be used as a name, on ` function test1<const T>(create: () => T): T { return null as unknown as (T); } ` |
| `types/typeParameters/typeParameterLists/typeParameterConstModifiersReverseMappedTypes.ts` | not supported | `const` is a reserved keyword and can't be used as a name, on ` function test1<const T>(obj: { ` |
| `types/typeParameters/typeParameterLists/typeParameterConstModifiersWithIntersection.ts` | not supported | intersection types |
| `types/typeParameters/typeParameterLists/typeParameterDirectlyConstrainedToItself.ts` | not supported | expected `,` or `>`, on ` class C<T extends T> { } ` |
| `types/typeParameters/typeParameterLists/typeParameterIndirectlyConstrainedToItself.ts` | not supported | expected `,` or `>`, on ` class C<U extends T, T extends U> { } ` |
| `types/typeParameters/typeParameterLists/typeParametersAvailableInNestedScope.ts` | not supported | expected type, on ` x: <U>(a: U) => T = <U>(a: U) => { ` |
| `types/typeParameters/typeParameterLists/typeParametersAvailableInNestedScope3.ts` | not supported | expected type, on ` function foo<T>(v: T): { a: <T>(a: T) => T; b: () => T; c: <T>(v: T) => { a: ... ` |
| `types/typeParameters/typeParameterLists/typeParameterUsedAsConstraint.ts` | not supported | expected `,` or `>`, on ` class C<T, U extends T> { } ` |
| `types/typeParameters/typeParameterLists/varianceAnnotations.ts` | not supported | expected `,` or `>`, on ` type Covariant<out T> = { ` |
| `types/typeParameters/typeParameterLists/varianceAnnotationsWithCircularlyReferencesError.ts` | not supported | `in` is a reserved keyword and can't be used as a name, on ` type T1<in in> = T1 // Error: circularly references ` |
| `types/typeRelationships/apparentType/apparentTypeSubtyping.ts` | not supported | expected `,` or `>`, on ` class Base<U extends String> { ` |
| `types/typeRelationships/apparentType/apparentTypeSupertype.ts` | not supported | expected `,` or `>`, on ` class Derived<U extends String> extends Base { // error ` |
| `types/typeRelationships/assignmentCompatibility/anyAssignabilityInInheritance.ts` | not supported | `any` is not supported, on ` [x: string]: any; ` |
| `types/typeRelationships/assignmentCompatibility/anyAssignableToEveryType.ts` | not supported | `any` is not supported, on ` let a: any = null as unknown as (any); ` |
| `types/typeRelationships/assignmentCompatibility/anyAssignableToEveryType2.ts` | not supported | `any` is not supported, on ` [x: string]: any; ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithCallSignatures.ts` | not supported | expected field name in object type, on ` let a: { (x: number): void } = null as unknown as ({ (x: number): void }); ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithCallSignatures3.ts` | not supported | expected field name in object type, on ` (x: number): number[]; ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithCallSignatures4.ts` | not supported | expected `;` after expression, on ` namespace Errors { ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithCallSignatures5.ts` | not supported | expected type, on ` let a: <T>(x: T) => T[] = null as unknown as (<T>(x: T) => T[]); ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithCallSignatures6.ts` | not supported | expected type, on ` a: <T>(x: T) => T[]; ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithConstructSignatures.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithConstructSignatures2.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithConstructSignatures3.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithConstructSignatures4.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithConstructSignatures5.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithConstructSignatures6.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithConstructSignaturesWithOptionalParameters.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithDiscriminatedUnion.ts` | not supported | expected `;` after expression, on ` namespace Example1 { ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithEnumIndexer.ts` | not supported | index signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithGenericCallSignatures.ts` | not supported | expected type, on ` let f: <S extends { p: string }[]>(x: S) => void = null as unknown as (<S ext... ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithGenericCallSignatures2.ts` | not supported | generic call signatures are not yet supported, on ` <T>(x: T, ...y: T[][]): void ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithGenericCallSignatures3.ts` | not supported | generic call signatures are not yet supported, on ` <U>(f: (x: T) => (y: S) => U): U ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithGenericCallSignatures4.ts` | not supported | expected type, on ` let x: <T extends I2<T>>(z: T) => void = null as unknown as (<T extends I2<T>... ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithGenericCallSignaturesWithOptionalParameters.ts` | not supported | expected `;` after expression, on ` namespace ClassTypeParam { ` |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithNumericIndexer.ts` | not supported | index signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithNumericIndexer2.ts` | not supported | index signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithNumericIndexer3.ts` | not supported | index signatures |
| `types/typeRelationships/assignmentCompatibility/assignmentCompatWithObjectMembers.ts` | not supported | expected `;` after expression, on ` namespace SimpleTypes { ` |
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
| `types/typeRelationships/assignmentCompatibility/constructSignatureAssignabilityInInheritance.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/constructSignatureAssignabilityInInheritance2.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/constructSignatureAssignabilityInInheritance3.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/constructSignatureAssignabilityInInheritance4.ts` | not supported | construct signatures |
| `types/typeRelationships/assignmentCompatibility/constructSignatureAssignabilityInInheritance5.ts` | not supported | expected type, on ` a: new (x: number) => number[]; ` |
| `types/typeRelationships/assignmentCompatibility/constructSignatureAssignabilityInInheritance6.ts` | not supported | expected type, on ` a: new <T>(x: T) => T[]; ` |
| `types/typeRelationships/assignmentCompatibility/enumAssignability.ts` | not supported | expected `;` after expression, on ` namespace Others { ` |
| `types/typeRelationships/assignmentCompatibility/enumAssignabilityInInheritance.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `types/typeRelationships/assignmentCompatibility/everyTypeAssignableToAny.ts` | not supported | `any` |
| `types/typeRelationships/assignmentCompatibility/genericCallWithObjectTypeArgsAndInitializers.ts` | not supported | expected `,` or `>`, on ` function foo3<T extends Number>(x: T = 1): void { } // error ` |
| `types/typeRelationships/assignmentCompatibility/intersectionIncludingPropFromGlobalAugmentation.ts` | not supported | expected `;` after type alias body, on ` type Test2 = Test1 & { optional?: unknown }; ` |
| `types/typeRelationships/assignmentCompatibility/nullAssignableToEveryType.ts` | not supported | `any` is not supported, on ` let f: any = null; ` |
| `types/typeRelationships/assignmentCompatibility/nullAssignedToUndefined.ts` | the port changes what it checks | `tsc` then reports TS2322 |
| `types/typeRelationships/assignmentCompatibility/typeParameterAssignability2.ts` | not supported | expected `,` or `>`, on ` function foo<T, U extends T>(t: T, u: U): void { ` |
| `types/typeRelationships/assignmentCompatibility/typeParameterAssignability3.ts` | not supported | expected `,` or `>`, on ` function foo<T extends Foo, U extends Foo>(t: T, u: U): void { ` |
| `types/typeRelationships/bestCommonType/bestCommonTypeOfTuple2.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` interface base1 { i } ` |
| `types/typeRelationships/bestCommonType/functionWithMultipleReturnStatements.ts` | not supported | expected `,` or `>`, on ` function f8<T extends U, U extends V, V>(x: T, y: U): U { ` |
| `types/typeRelationships/bestCommonType/heterogeneousArrayLiterals.ts` | not supported | expected `;` after expression, on ` namespace Derived { ` |
| `types/typeRelationships/comparable/equalityWithIntersectionTypes01.ts` | not supported | intersection types |
| `types/typeRelationships/comparable/switchCaseWithIntersectionTypes01.ts` | not supported | intersection types |
| `types/typeRelationships/comparable/typeAssertionsWithIntersectionTypes01.ts` | not supported | intersection types |
| `types/typeRelationships/instanceOf/narrowingConstrainedTypeVariable.ts` | not supported | expected `,` or `>`, on ` function f1<T extends C>(v: T \| string): void { ` |
| `types/typeRelationships/recursiveTypes/arrayLiteralsWithRecursiveGenerics.ts` | not supported | `as` to `List<number>` is not yet supported: class types aren't yet supported as `as` targets, on ` let list: List<number> = null as unknown as (List<number>); ` |
| `types/typeRelationships/recursiveTypes/infiniteExpansionThroughInstantiation.ts` | not supported | `let` declaration requires an initializer, on ` let ownerList!: OwnerList<T> = null as unknown as (OwnerList<T>); ` |
| `types/typeRelationships/recursiveTypes/infiniteExpansionThroughInstantiation2.ts` | not supported | expected `,` or `>`, on ` interface AA<T extends AA<T>> // now an error due to referencing type paramet... ` |
| `types/typeRelationships/recursiveTypes/nominalSubtypeCheckOfTypeParameter.ts` | checks too little | 0 after the port |
| `types/typeRelationships/recursiveTypes/nominalSubtypeCheckOfTypeParameter2.ts` | checks too little | 0 after the port |
| `types/typeRelationships/recursiveTypes/recursiveTypeInGenericConstraint.ts` | not supported | expected `,` or `>`, on ` class Foo<T extends G<T>> { // error, constraint referencing itself ` |
| `types/typeRelationships/recursiveTypes/recursiveTypesUsedAsFunctionParameters.ts` | not supported | expected `:` and return type, on ` function foo<T>(x: List<T>); ` |
| `types/typeRelationships/subtypesAndSuperTypes/stringLiteralTypeIsSubtypeOfString.ts` | not supported | expected `:` and return type, on ` function f1(x: 'a'); ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfAny.ts` | not supported | `any` is not supported, on ` [x: string]: any; ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameter.ts` | not supported | expected `;` after expression, on ` namespace f { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameterWithConstraints.ts` | not supported | expected `,` or `>`, on ` class D1<T extends U, U> extends C3<T> { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameterWithConstraints2.ts` | not supported | expected `,` or `>`, on ` function f1<T extends U, U>(x: T, y: U): void { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameterWithConstraints3.ts` | not supported | expected `,` or `>`, on ` function f<T extends U, U, V>(t: T, u: U, v: V): void { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameterWithConstraints4.ts` | not supported | expected `,` or `>`, on ` function f<T extends Foo, U extends Foo, V>(t: T, u: U, v: V): void { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfTypeParameterWithRecursiveConstraints.ts` | not supported | expected `,` or `>`, on ` function f<T extends Foo<U>, U extends Foo<T>, V extends Foo<V>>(t: T, u: U, ... ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypesOfUnion.ts` | not supported | expected `;` after expression, on ` namespace f { export let bar = 1; } ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithCallSignatures.ts` | not supported | expected `;` after expression, on ` namespace CallSignature { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithCallSignatures2.ts` | not supported | `any` is not supported, on ` function foo1(a: any): any { return null as unknown as (any); } ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithCallSignatures3.ts` | not supported | expected `;` after expression, on ` namespace Errors { ` |
| `types/typeRelationships/subtypesAndSuperTypes/subtypingWithCallSignatures4.ts` | not supported | expected type, on ` function foo1(a: <T>(x: T) => T[]): void { } ` |
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
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentity.ts` | not supported | expected `:` and return type, on ` function foo1(x: A); ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentity2.ts` | not supported | expected `:` and return type, on ` function foo5(x: A); ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithCallSignatures.ts` | not supported | expected `:` and return type, on ` function foo1(x: A); ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithCallSignatures2.ts` | not supported | expected `:` and return type, on ` function foo1(x: A); ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithCallSignatures3.ts` | not supported | expected field name in object type, on ` let a: { (x: string): string } = null as unknown as ({ (x: string): string }); ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithCallSignaturesDifferingParamCounts.ts` | not supported | expected `:` and return type, on ` function foo1(x: A); ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithCallSignaturesDifferingParamCounts2.ts` | not supported | expected field name in object type, on ` let a: { (x: string, y: string): string } = null as unknown as ({ (x: string,... ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithCallSignaturesWithOverloads.ts` | not supported | expected `{`, on ` foo(x: number): number; ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithComplexConstraints.ts` | not supported | generic call signatures are not yet supported, on ` <T extends { ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithConstructSignatures.ts` | not supported | construct signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithConstructSignatures2.ts` | not supported | construct signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithConstructSignaturesDifferingParamCounts.ts` | not supported | construct signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignatures.ts` | not supported | expected `:` after field name, on ` let a: { foo<T>(x: T): T } = null as unknown as ({ foo<T>(x: T): T }); ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignatures2.ts` | not supported | expected `:` after field name, on ` let a: { foo<T, U>(x: T, y: U): T } = null as unknown as ({ foo<T, U>(x: T, y... ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesDifferingByConstraints.ts` | not supported | expected `,` or `>`, on ` foo<T extends Date>(x: T): string { return null; } ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesDifferingByConstraints2.ts` | not supported | expected `,` or `>`, on ` foo<T extends U, U extends Date>(x: T, y: U): string { return null; } ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesDifferingByConstraints3.ts` | not supported | expected `,` or `>`, on ` foo<T extends U, U extends One>(x: T, y: U): string { return null; } ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesDifferingByReturnType.ts` | not supported | expected `:` after field name, on ` let a: { foo<T>(x: T): T } = null as unknown as ({ foo<T>(x: T): T }); ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesDifferingByReturnType2.ts` | not supported | expected `,` or `>`, on ` foo<T extends Date>(x: T): string { return null; } ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesDifferingTypeParameterCounts.ts` | not supported | expected `:` after field name, on ` let a: { foo<Z, A, B, C, D>(x: Z): Z } = null as unknown as ({ foo<Z, A, B, C... ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesDifferingTypeParameterCounts2.ts` | not supported | generic call signatures are not yet supported, on ` <Y, Z, A, B>(x: Y): Y; ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesDifferingTypeParameterNames.ts` | not supported | expected `:` after field name, on ` let a: { foo<Z>(x: Z): Z } = null as unknown as ({ foo<Z>(x: Z): Z }); ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesOptionalParams.ts` | not supported | expected `:` after field name, on ` let a: { foo<T>(x: T, y?: T): T } = null as unknown as ({ foo<T>(x: T, y?: T)... ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesOptionalParams2.ts` | not supported | expected `:` after field name, on ` let a: { foo<T, U>(x: T, y?: U): T } = null as unknown as ({ foo<T, U>(x: T, ... ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithGenericCallSignaturesOptionalParams3.ts` | not supported | expected `:` after field name, on ` let a: { foo<T, U>(x: T, y?: U): T } = null as unknown as ({ foo<T, U>(x: T, ... ` |
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
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithOptionality.ts` | not supported | expected `:` and return type, on ` function foo2(x: I); ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithPrivates.ts` | not supported | expected `:` and return type, on ` function foo1(x: A); ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithPrivates2.ts` | not supported | expected `:` and return type, on ` function foo1(x: C<string>); ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithPrivates3.ts` | not supported | expected `(` to start a method signature or `:` to start a property, on ` interface T2 { z } ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithPublics.ts` | not supported | expected `:` and return type, on ` function foo1(x: A); ` |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithStringIndexers.ts` | not supported | index signatures |
| `types/typeRelationships/typeAndMemberIdentity/objectTypesIdentityWithStringIndexers2.ts` | not supported | index signatures |
| `types/typeRelationships/typeAndMemberIdentity/primtiveTypesAreIdentical.ts` | not supported | expected `:` and return type, on ` function foo1(x: number); ` |
| `types/typeRelationships/typeAndMemberIdentity/typeParametersAreIdenticalToThemselves.ts` | not supported | expected `:` and return type, on ` function foo1<T>(x: T); ` |
| `types/typeRelationships/typeAndMemberIdentity/unionTypeIdentity.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `types/typeRelationships/typeInference/bivariantInferences.ts` | not supported | `this` is a reserved keyword and can't be used as a name, on ` equalsShallow<T>(this: ReadonlyArray<T>, other: ReadonlyArray<T>): boolean; ` |
| `types/typeRelationships/typeInference/contextualSignatureInstantiation.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `types/typeRelationships/typeInference/discriminatedUnionInference.ts` | not supported | expected field name in object type, on ` type Foo<A> = { type: "foo", (): A[] }; ` |
| `types/typeRelationships/typeInference/genericCallToOverloadedMethodWithOverloadedArguments.ts` | not supported | expected `;` after expression, on ` namespace m1 { ` |
| `types/typeRelationships/typeInference/genericCallTypeArgumentInference.ts` | not supported | construct signatures |
| `types/typeRelationships/typeInference/genericCallWithConstraintsTypeArgumentInference.ts` | not supported | construct signatures |
| `types/typeRelationships/typeInference/genericCallWithConstraintsTypeArgumentInference2.ts` | not supported | expected `,` or `>`, on ` function foo<T, U extends T>(t: T): U { ` |
| `types/typeRelationships/typeInference/genericCallWithConstructorTypedArguments5.ts` | not supported | expected type, on ` function foo<T, U>(arg: { cb: new(t: T) => U }): U { ` |
| `types/typeRelationships/typeInference/genericCallWithFunctionTypedArguments2.ts` | not supported | construct signatures |
| `types/typeRelationships/typeInference/genericCallWithFunctionTypedArguments3.ts` | not supported | expected field name in object type, on ` (x: boolean): boolean; ` |
| `types/typeRelationships/typeInference/genericCallWithFunctionTypedArguments4.ts` | not supported | construct signatures |
| `types/typeRelationships/typeInference/genericCallWithGenericSignatureArguments2.ts` | not supported | expected `;` after expression, on ` namespace onlyT { ` |
| `types/typeRelationships/typeInference/genericCallWithGenericSignatureArguments3.ts` | not supported | `let` declaration requires an initializer, on ` let r!: (x: T) => T = null as unknown as ((x: T) => T); ` |
| `types/typeRelationships/typeInference/genericCallWithNonSymmetricSubtypes.ts` | not supported | `as` to `T` is not yet supported: generic type parameters are erased at runtime, on ` let r: T = null as unknown as (T); ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgs.ts` | not supported | `let` declaration requires an initializer, on ` let x!: T = null as unknown as (T); ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgs2.ts` | not supported | expected `,` or `>`, on ` function f<T extends Base, U extends Base>(a: { x: T; y: U }): (T \| U)[] { ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndConstraints.ts` | not supported | expected `,` or `>`, on ` function foo<T extends { x: string }>(t: X<T>, t2: X<T>): T { ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndConstraints2.ts` | not supported | expected `,` or `>`, on ` function f<T extends Base>(x: { foo: T; bar: T }): T { ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndConstraints3.ts` | not supported | expected `,` or `>`, on ` function f<T extends Base>(a: { x: T; y: T }): T { ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndConstraints4.ts` | not supported | expected `,` or `>`, on ` function foo<T, U extends T>(t: T, t2: U): (x: T) => U { ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndConstraints5.ts` | not supported | expected `,` or `>`, on ` function foo<T, U extends T>(t: T, t2: U): (x: T) => U { ` |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndIndexers.ts` | not supported | index signatures |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndIndexersErrors.ts` | not supported | index signatures |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndNumericIndexer.ts` | not supported | index signatures |
| `types/typeRelationships/typeInference/genericCallWithOverloadedConstructorTypedArguments.ts` | not supported | construct signatures |
| `types/typeRelationships/typeInference/genericCallWithOverloadedConstructorTypedArguments2.ts` | not supported | construct signatures |
| `types/typeRelationships/typeInference/genericCallWithOverloadedFunctionTypedArguments.ts` | not supported | expected `;` after expression, on ` namespace NonGenericParameter { ` |
| `types/typeRelationships/typeInference/genericCallWithOverloadedFunctionTypedArguments2.ts` | not supported | expected `;` after expression, on ` namespace NonGenericParameter { ` |
| `types/typeRelationships/typeInference/genericClassWithFunctionTypedMemberArguments.ts` | not supported | expected `;` after expression, on ` namespace ImmediatelyFix { ` |
| `types/typeRelationships/typeInference/genericClassWithObjectTypeArgsAndConstraints.ts` | not supported | expected `;` after expression, on ` namespace Class { ` |
| `types/typeRelationships/typeInference/genericContextualTypes1.ts` | not supported | expected type, on ` const f00: <A>(x: A) => A[] = list; ` |
| `types/typeRelationships/typeInference/genericContextualTypes2.ts` | not supported | expected `;` after type alias body, on ` type LowInfer<T> = T & {}; ` |
| `types/typeRelationships/typeInference/genericContextualTypes3.ts` | not supported | expected `;` after type alias body, on ` type LowInfer<T> = T & {}; ` |
| `types/typeRelationships/typeInference/genericFunctionParameters.ts` | not supported | expected type, on ` function f1<T>(cb: <S>(x: S) => T): T { return null as unknown as (T); } ` |
| `types/typeRelationships/typeInference/indexSignatureTypeInference.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `types/typeRelationships/typeInference/keyofInferenceIntersectsResults.ts` | not supported | expected `,` or `>`, on ` function foo<T = X>(x: keyof T, y: keyof T): T { return null as unknown as (T... ` |
| `types/typeRelationships/typeInference/keyofInferenceLowerPriorityThanReturn.ts` | not supported | expected `;` after expression, on ` declare class Write { ` |
| `types/typeRelationships/typeInference/noInfer.ts` | not supported | expected type, on `` type T04 = NoInfer<`foo${string}`>; `` |
| `types/typeRelationships/typeInference/noInferRedeclaration.ts` | not supported | expected `)`, on ` export const f = <T>(x: T, y: NoInfer<T>) => x; ` |
| `types/typeRelationships/typeInference/unionAndIntersectionInference1.ts` | not supported | intersection types |
| `types/typeRelationships/typeInference/unionAndIntersectionInference2.ts` | not supported | intersection types |
| `types/typeRelationships/typeInference/unionAndIntersectionInference3.ts` | not supported | intersection types |
| `types/typeRelationships/typeInference/unionTypeInference.ts` | not supported | expected `,` or `)`, on ` function f4<T>(x: string & T): T { return null as unknown as (T); } ` |
| `types/union/contextualTypeWithUnionTypeCallSignatures.ts` | not supported | duplicate call signature on interface `IWithCallSignatures4`, on ` (a: string, b: number): number; ` |
| `types/union/contextualTypeWithUnionTypeIndexSignatures.ts` | not supported | index signatures |
| `types/union/discriminatedUnionTypes3.ts` | not supported | expected type, on `` err: `${string} is wrong!` `` |
| `types/union/discriminatedUnionTypes4.ts` | not supported | expected type, on `` type: `${AnimalType.cat}`; `` |
| `types/union/unionTypeCallSignatures.ts` | not supported | expected field name in object type, on ` let unionOfDifferentReturnType: { (a: number): number; } \| { (a: number): Dat... ` |
| `types/union/unionTypeCallSignatures2.ts` | not supported | generic call signatures are not yet supported, on ` <T>(x: T[]): T[]; ` |
| `types/union/unionTypeCallSignatures5.ts` | not supported | `this` is a reserved keyword and can't be used as a name, on ` (this: void, b?: number): void; ` |
| `types/union/unionTypeCallSignatures6.ts` | not supported | `this` is a reserved keyword and can't be used as a name, on ` type F1 = (this: A) => void; ` |
| `types/union/unionTypeCallSignatures7.ts` | not supported | expected `,` or `>`, on ` interface Callable<Name extends string> { ` |
| `types/union/unionTypeConstructSignatures.ts` | not supported | construct signatures |
| `types/union/unionTypeEquivalence.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `types/union/unionTypeIndexSignature.ts` | not supported | index signatures |
| `types/union/unionTypePropertyAccessibility.ts` | not supported | `protected` is not supported, on ` protected member: string; ` |
| `types/union/unionTypeReduction.ts` | not supported | parameter requires a type annotation, on ` (q): boolean; ` |
| `types/union/unionTypeWithIndexSignature.ts` | not supported | index signatures |
| `types/uniqueSymbol/` | not supported | `Symbol` |
| `types/witness/witness.ts` | the port changes what it checks | it renames the repeated `var` declarations whose types `tsc` checks (TS2403) |
| `typings/typingsLookup1.ts` | not supported | JSON modules |
| `typings/typingsLookup2.ts` | not supported | JSON modules |
| `typings/typingsLookup3.ts` | not supported | JSON modules |
| `typings/typingsLookup4.ts` | not supported | JSON modules |
| `typings/typingsLookupAmd.ts` | not supported | `node_modules` packages |
| `typings/typingsSuggestion1.ts` | not supported | JSON modules |
| `typings/typingsSuggestion2.ts` | not supported | JSON modules |
| `typings/typingsSuggestionBun1.ts` | not supported | JSON modules |
| `typings/typingsSuggestionBun2.ts` | not supported | JSON modules |
