# submilli-front-end

The contract between a Submilli front end and the Submilli engine.

A front end parses and checks source. It hands the engine a `TypedAst`, and
for a package a `PackageDeclaration`. This crate holds those types, the
`Type` and `Diagnostic` values they carry, and the helpers both sides must
compute the same way. The engine (`submilli-engine`) and ts-rust both depend
on it.

## Class member visibility

`Visibility::Protected` carries TypeScript's `protected` modifier through
`TypedAst` and `PackageDeclaration`. Front ends enforce TypeScript access rules
inside the declaring class and its subclasses, including subclasses in other
packages. A protected constructor permits subclassing across packages and
restricts direct construction to the class and its subclasses.

The engine gives protected members the same layout, inheritance, dispatch, and
enumeration as public members. Package declarations serialize the visibility as
`Protected`, and the package docs and `.d.ts` render it as `protected`.
This contract is available to front ends that implement TypeScript's access
rules. The built-in front end continues to reject `protected` in source.
