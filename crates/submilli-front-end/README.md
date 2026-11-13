# submilli-front-end

The contract between a Submilli front end and the Submilli engine.

A front end parses and checks source. It hands the engine a `TypedAst`, and
for a package a `PackageDeclaration`. This crate holds those types, the
`Type` and `Diagnostic` values they carry, and the helpers both sides must
compute the same way. The engine (`submilli-engine`) and ts-rust both depend
on it.
