# Writing the Submilli book

Each chapter should leave the reader able to explain something or do something
they could not before. Write for a person learning Submilli and an agent using
the chapter to write code. Both need clear explanations, precise rules, and
examples they can trust.

## Start with a chapter brief

Before drafting, answer these questions in a short working note:

- **Purpose:** Why does this chapter exist? One sentence.
- **Starting point:** What does the reader already know? Name prerequisites.
- **Understanding:** What should the reader be able to explain afterward?
  Choose three to five concrete outcomes. Avoid vague goals such as
  “understand the language.”
- **Action:** What should the reader be able to write, run, or decide?
- **Boundaries:** What belongs here, and what belongs in another chapter?
- **Evidence:** Which examples will demonstrate the outcomes? Which repository
  sources or tests will we use to check the claims?

Use the brief to choose the sections and their order. Keep it with the authoring
notes; the chapter itself should open with the reader's problem and why it matters.

## Draft for understanding

Introduce an idea, show it in a small example, and explain the result. State
exceptions beside the rule they qualify. Add detail when the reader needs it to
take the next step. A reference table can collect facts that are easier to look up
than to read as prose.

For humans, explain why a rule matters and connect each idea to the previous one.
For agents, name the feature explicitly, use consistent terminology, and state
the exact behavior. Distinguish supported syntax, runtime behavior, and advice.
Make headings descriptive enough that a reader retrieving one section can find
the relevant rule.

Write as a knowledgeable colleague explaining a real task. Use ordinary words,
concrete subjects, and active verbs. Let sentence length follow the thought.
Remove stock openings, sales claims, repeated summaries, and words such as
“simply” or “obviously” that assume the reader already understands. Natural prose
comes from having something specific to say.

## Watch for the curse of knowledge

The curse of knowledge is forgetting what it was like not to know something.
As authors, we know how Submilli fits together. Readers may not know the terms,
connections, or intermediate steps that make a sentence obvious to us. Check
what each explanation assumes against the chapter's stated starting point.

The remedy isn't always a longer explanation. Describe the behavior in terms
the reader already understands. Define a new term briefly when they need it;
otherwise, save the technical name and its details for the chapter where they
become useful. Expanding an acronym alone rarely explains the idea.

For example, “Successful MCP results omit logs” assumes the reader knows what
MCP is and how an agent calls Submilli. We can explain the relevant behavior
without introducing the protocol: “When an agent framework calls Submilli, a
successful run returns only the result from `main`. The framework can retrieve
the logs through another tool call.”

During review, ask: what must the reader already know for this sentence to make
sense? If we haven't established it, add the missing connection or rewrite the
sentence around the reader's task. Keep enough context for a reader or agent
arriving directly at this section.

## Eleven rules, adapted from Twain

These are documentation adaptations of the
[eleven-rule selection from *Fenimore Cooper's Literary Offenses*](https://homepages.bluffton.edu/~bergerd/classes/twain.html).
Twain's original list is longer; the wording below is ours.

1. **Arrive somewhere.** Deliver the outcome promised in the chapter brief.
2. **Make every part earn its place.** Each section should advance that outcome.
3. **Respect the reader's judgment.** Support claims; don't dress guesses as facts.
4. **Make examples credible.** Use plausible tasks and verify their behavior.
5. **Say exactly what you mean.** Replace broad claims with explicit rules.
6. **Choose the precise word.** Keep names and technical terms consistent.
7. **Cut excess.** Remove sentences that add no meaning.
8. **Keep necessary detail.** Include prerequisites, exceptions, and expected results.
9. **Take care with form.** Organize headings, examples, and links deliberately.
10. **Use sound grammar.** Make relationships between ideas unambiguous.
11. **Keep the style straightforward.** Prefer direct explanations to ornament.

## Check before calling a chapter complete

Read it against the brief: can the reader now explain the promised ideas and
perform the promised action? Check language claims against the implementation and
tests. Run runnable examples; label fragments and examples of rejected code, and
explain their expected result or error. Resolve uncertain behavior before presenting
it as a rule. File confirmed compiler bugs in Linear's interpreter project.
Write the book and agent prompt for the intended behavior after those fixes;
keep bug caveats and workarounds in the issues. Check links and build the documentation site using the
[site instructions](README.md#build-and-preview).

Then read the prose aloud. Rewrite anything you would struggle to say to a
colleague. Cut repetition without removing details needed to use the feature.

## First brief: chapter 4, “The language”

- **Purpose:** Enable readers to write Submilli programs and recognize where
  familiar TypeScript assumptions need checking.
- **Starting point:** Basic JavaScript or TypeScript knowledge and the ability to
  run a program from the quickstart.
- **Understanding:** Explain which TypeScript constructs are supported; how
  values and types relate; how missing values are represented; and which checks
  happen at compile time versus runtime.
- **Action:** Write and run a small program using typed data, a function, and
  control flow; diagnose and correct a representative language error.
- **Boundaries:** Cover syntax and language semantics, using HTTP and filesystem
  calls for a worked example with enough setup to run it. Leave the standard-library
  catalog to chapter 5, and blueprint configuration and permissions to their
  respective chapters. Link to them where needed.
- **Evidence:** Start with one runnable program, then focused examples of the
  important TypeScript differences. Verify them against interpreter fixtures,
  conformance cases, and implementation; consult `llm-prompt.md` for the guidance
  agents currently receive.
