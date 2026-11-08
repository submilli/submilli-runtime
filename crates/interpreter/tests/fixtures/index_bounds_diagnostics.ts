function main(): void {
  const parts = "Thursday, May 18, 2023".split("-");
  try {
    console.log(parts[1]);
    assert(false, "array read must throw");
  } catch (e: MissingValueError) {
    assert(e.message === "index 1 is out of range for `parts` (length 1); use `parts[1] ?? …` or `parts.at(1)`", e.message);
  }

  const text = "ab";
  try {
    console.log(text[5]);
    assert(false, "string read must throw");
  } catch (e: MissingValueError) {
    assert(e.message === "index 5 is out of range for `text` (length 2); use `text[5] ?? …` or `text.at(5)`", e.message);
  }

  const xs = [1];
  try {
    xs[3] = 2;
    assert(false, "array write must throw");
  } catch (e: RangeError) {
    assert(e.message === "index 3 is out of range for `xs` (length 1); use `xs.push(value)` to append instead", e.message);
  }

  try {
    console.log("a-b".split("-")[4]);
    assert(false, "complex receiver must throw");
  } catch (e: MissingValueError) {
    assert(e.message === "index 4 is out of range (length 2); use `?? …` at the read, or `.at(4)`", e.message);
  }

  const data = { items: [1], text: "\uD800x" };
  try {
    console.log(data.items[2]);
    assert(false, "short property path must throw");
  } catch (e: MissingValueError) {
    assert(e.message === "index 2 is out of range for `data.items` (length 1); use `data.items[2] ?? …` or `data.items.at(2)`", e.message);
  }
  try {
    console.log(data.text[2]);
    assert(false, "UTF-16 length must be reported");
  } catch (e: MissingValueError) {
    assert(e.message === "index 2 is out of range for `data.text` (length 2); use `data.text[2] ?? …` or `data.text.at(2)`", e.message);
  }

  const maybe: number[] | undefined = xs.length > 0 ? xs : undefined;
  try {
    console.log(maybe?.[4].toFixed(0));
    assert(false, "present optional receiver must throw");
  } catch (e: MissingValueError) {
    assert(e.message === "index 4 is out of range for `maybe` (length 1); use `maybe[4] ?? …` or `maybe.at(4)`", e.message);
  }

  try {
    console.log(xs[-1]);
    assert(false, "negative read must throw");
  } catch (e: MissingValueError) {
    assert(e.message === "index -1 is out of range for `xs` (length 1); use `xs[-1] ?? …` for a default, or `xs.at(-1)` to count from the end", e.message);
    assert(xs.at(-1) === 1, "at(-1) returns a value, not undefined");
  }
  try {
    console.log(text[NaN]);
    assert(false, "NaN read must throw");
  } catch (e: MissingValueError) {
    assert(e.message === "index NaN is out of range for `text` (length 2); use `text[NaN] ?? …` for a default, or `text.at(NaN)` to count from the end", e.message);
    assert(text.at(NaN) === "a", "at(NaN) reads zero");
  }

  try {
    xs[4]++;
    assert(false, "update must throw at the read");
  } catch (e: MissingValueError) {
    assert(e.message === "index 4 is out of range for `xs` (length 1); use `xs[4] ?? …` or `xs.at(4)`", e.message);
  }

  try {
    xs[4] = xs.push(2);
    assert(false, "write must use the length after the RHS");
  } catch (e: RangeError) {
    assert(e.message === "index 4 is out of range for `xs` (length 2); use `xs.push(value)` to append instead", e.message);
    assert(xs[1] === 2, "the RHS effect survives the throw");
  }

  const veryLongReceiverNameThatMustNotBeRepeatedInTheDiagnosticBecauseItExceedsSixtyFourBytes = [1];
  try {
    console.log(veryLongReceiverNameThatMustNotBeRepeatedInTheDiagnosticBecauseItExceedsSixtyFourBytes[3]);
    assert(false, "long receiver name must throw without a label");
  } catch (e: MissingValueError) {
    assert(e.message === "index 3 is out of range (length 1); use `?? …` at the read, or `.at(3)`", e.message);
  }

  const bytes = Uint8Array.fromArray([1]);
  try {
    bytes.with(3, 2);
    assert(false, "with must still throw");
  } catch (e: RangeError) {
    assert(e.message === "index 3 is out of range (length 1)", e.message);
  }

  const quoted = { "a-b": [1], "a.b": [1], "bad`path": "x", "line\nbreak": [1] };
  try {
    console.log(quoted["a-b"][2]);
    assert(false, "a quoted key must not become an invalid dotted path");
  } catch (e: MissingValueError) {
    assert(e.message === "index 2 is out of range (length 1); use `?? …` at the read, or `.at(2)`", e.message);
  }
  const maybeQuoted = xs.length > 0 ? quoted : undefined;
  try {
    console.log(quoted["a.b"][2]);
    assert(false, "a dotted key is one segment, not a dotted path");
  } catch (e: MissingValueError) {
    assert(e.message === "index 2 is out of range (length 1); use `?? …` at the read, or `.at(2)`", e.message);
  }
  try {
    console.log(maybeQuoted?.["a.b"]?.[2].toFixed(0));
    assert(false, "optional dotted quoted keys must not become a path");
  } catch (e: MissingValueError) {
    assert(e.message === "index 2 is out of range (length 1); use `?? …` at the read, or `.at(2)`", e.message);
  }
  try {
    console.log(maybeQuoted?.["bad`path"]?.[2].length);
    assert(false, "optional paths must reject punctuation too");
  } catch (e: MissingValueError) {
    assert(e.message === "index 2 is out of range (length 1); use `?? …` at the read, or `.at(2)`", e.message);
  }
  try {
    console.log(maybeQuoted?.["line\nbreak"]?.[2].toFixed(0));
    assert(false, "optional paths must reject newlines too");
  } catch (e: MissingValueError) {
    assert(e.message === "index 2 is out of range (length 1); use `?? …` at the read, or `.at(2)`", e.message);
  }

  const narrowed: { "a.b": number[] | undefined } = { "a.b": [1] };
  if (narrowed["a.b"] !== undefined) {
    try {
      console.log(narrowed["a.b"][2]);
      assert(false, "narrowed paths must validate the original key");
    } catch (e: MissingValueError) {
      assert(e.message === "index 2 is out of range (length 1); use `?? …` at the read, or `.at(2)`", e.message);
    }
  }
  const π = { données: [1] };
  try {
    console.log(π.données[2]);
    assert(false, "valid Unicode path must throw with its label");
  } catch (e: MissingValueError) {
    assert(e.message === "index 2 is out of range for `π.données` (length 1); use `π.données[2] ?? …` or `π.données.at(2)`", e.message);
  }
}
