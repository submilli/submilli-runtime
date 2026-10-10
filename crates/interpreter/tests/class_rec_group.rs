//! SUB-480: WasmGC rec-group emission for `class` declarations.
//!
//! These exercise type emission only — method dispatch, `new`, and constructors
//! are deferred (SUB-483/484/486), so no class is instantiated here. The module
//! must still validate, and the per-class struct + vtable types must appear in
//! the emitted rec group with the expected `$ObjectShape`/`$VTable` nesting.

use submilli_engine::{FileId, compile_script};
use wasmparser::{CompositeInnerType, Parser, Payload, Validator};

fn compile(source: &str) -> Vec<u8> {
    compile_script(source, "script.subm", FileId(0), &[], &[])
        .expect("class module compiles without diagnostics")
        .wasm
}

/// (field_count, has_supertype) for every struct type across all rec groups.
fn struct_shapes(bytes: &[u8]) -> Vec<(usize, bool)> {
    let mut out = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::TypeSection(reader) = payload.expect("payload") {
            for rec in reader {
                for sub in rec.expect("rec group").types() {
                    if let CompositeInnerType::Struct(s) = &sub.composite_type.inner {
                        out.push((s.fields.len(), sub.supertype_idx.is_some()));
                    }
                }
            }
        }
    }
    out
}

const ANIMAL_DOG: &str = r#"
class Animal {
  name: string;
  private sound: string;

  constructor(name: string, sound: string) {
    this.name = name;
    this.sound = sound;
  }

  speak(): string {
    return this.name + " says " + this.sound;
  }
}

class Dog extends Animal {
  private tricks: string[];

  constructor(name: string) {
    super(name, "woof");
    this.tricks = [];
  }

  learn(trick: string): void {
    this.tricks.push(trick);
  }

  speak(): string {
    return this.name + " barks";
  }
}

function main(): void {}
"#;

#[test]
fn animal_dog_module_validates() {
    let bytes = compile(ANIMAL_DOG);
    Validator::new()
        .validate_all(&bytes)
        .expect("class rec group + globals + stubs validate");
}

#[test]
fn animal_dog_rec_group_shapes() {
    let bytes = compile(ANIMAL_DOG);
    let shapes = struct_shapes(&bytes);

    let mut type_index = 0;
    let mut pairs = Vec::new();
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::TypeSection(reader) = payload.expect("payload") {
            for rec in reader {
                let rec = rec.expect("rec group");
                let members = rec.types().collect::<Vec<_>>();
                if let [vtable, instance] = members.as_slice()
                    && let CompositeInnerType::Struct(vtable_shape) = &vtable.composite_type.inner
                    && let CompositeInnerType::Struct(instance_shape) =
                        &instance.composite_type.inner
                {
                    pairs.push((
                        type_index,
                        vtable_shape.fields.len(),
                        instance_shape.fields.len(),
                        vtable.supertype_idx,
                        instance.supertype_idx,
                    ));
                }
                type_index += members.len() as u32;
            }
        }
    }
    // Class declarations emit (vtable, instance) pairs. Identify Animal and
    // Dog through both subtype links, so unrelated intrinsic shapes cannot
    // satisfy the assertions. Six vtable header fields precede methods.
    let animal = pairs
        .iter()
        .find(|pair| pair.1 == 7 && pair.2 == 5)
        .expect("Animal vtable + instance pair");
    let dog = pairs
        .iter()
        .find(|pair| {
            pair.3
                .is_some_and(|parent| parent.as_module_index() == Some(animal.0))
                && pair
                    .4
                    .is_some_and(|parent| parent.as_module_index() == Some(animal.0 + 1))
        })
        .expect("Dog pair subtypes both Animal types");
    assert_eq!((dog.1, dog.2), (8, 5), "unexpected Dog layout: {shapes:?}");
}

#[test]
fn single_class_with_fields_validates() {
    let bytes = compile(
        r#"
        class Point {
          x: number;
          y: number;
          constructor(x: number, y: number) { this.x = x; this.y = y; }
          dist(): number { return this.x + this.y; }
        }
        function main(): void {}
    "#,
    );
    Validator::new()
        .validate_all(&bytes)
        .expect("single class validates");
    assert!(struct_shapes(&bytes).contains(&(5, true)));
}

#[test]
fn class_with_methods_and_override_validates() {
    // Real method bodies (SUB-484) + an override on a subclass. Dog isn't
    // constructible yet (super → SUB-487), but its `speak` override body must
    // still emit and the module must validate.
    let bytes = compile(
        r#"
        class Animal {
          private sound: string;
          constructor(sound: string) { this.sound = sound; }
          speak(): string { return this.sound; }
        }
        class Dog extends Animal {
          constructor() { super("woof"); }
          speak(): string { return "bark"; }
        }
        function main(): void {}
    "#,
    );
    Validator::new()
        .validate_all(&bytes)
        .expect("class with methods + override validates");
}

#[test]
fn class_with_no_methods_validates() {
    let bytes = compile(
        r#"
        class Empty {
          value: number = 0;
        }
        function main(): void {}
    "#,
    );
    Validator::new()
        .validate_all(&bytes)
        .expect("a class with no methods (vtable = 4 universal slots) validates");
}
