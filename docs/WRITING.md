# Writing the Submilli book

Each chapter should leave the reader able to explain something or do something
they could not before. Write for two readers: a person learning Submilli, and an
agent evaluating it on someone's behalf — deciding whether it fits their system
and checking whether its claims hold. Both need clear explanations, precise
rules, and examples they can trust.

The agent that writes Submilli programs is a different reader. It gets the
Submilli skill and `llm-prompt.md`, which are written for that job. Reference
material that only a code-writing agent needs belongs there.

## The shape of the book

The book follows [Diátaxis](https://diataxis.fr/): every page is one of four
types, and the type decides how it is written. The parts are a single reading
line: each page assumes only what earlier pages established. Blueprints come
before packages everywhere, the policy first and then the operation it governs.
One scenario runs through the book: the Acme support agent, its billing
package, the customer `cus_northwind`, and the injected ticket.

| Part | Folder | Type | Read |
| --- | --- | --- | --- |
| 1 · Start here | `docs/part-1-start-here/` | Why and the four concepts are [explanation](https://diataxis.fr/explanation/); Install is a [how-to](https://diataxis.fr/how-to-guides/); Quickstart is a [tutorial](https://diataxis.fr/tutorials/) | In order |
| 2 · Blueprints | `docs/part-2-blueprints/` | How-to | One page, for a task |
| 3 · Packages | `docs/part-3-packages/` | How-to | One page, for a task |
| 4 · Server | `docs/part-4-server/` | How-to | One page, for a task |
| 5 · Tutorials | `docs/part-5-tutorials/` | Tutorial | In order, by group |
| 6 · Reference | `docs/part-6-reference/` | [Reference](https://diataxis.fr/reference/) | Looked up |

A page's URL comes from
its `slug` frontmatter, not its path; its position comes from `sidebar.order`.
A folder inside a part is a sidebar group, named in `astro.config.mjs`.

### Explanation

Say what the thing *is* before what it does, with an analogy the reader
already holds (a blueprint is to a session what a blueprint is to a house).
Then list its main parts and say which one the page is about. Explain why by
contrast with what the reader uses today, named concretely. Put the concept in
the heading, so the argument reads in the table of contents. Order ideas so
each answers the question the previous one raised.

### How-to

Open with why: one paragraph on the situation that makes the reader need this
page, then the task. Cover the whole lifecycle of the thing, author, test
locally, register, run on the server, and link to the page that owns each
detail. Put the bread and butter early and options late; advanced options
belong in reference. A how-to is read on its own: never explain its output by
what another page did. Don't hedge; one sentence beside the rule it qualifies
is enough.

### Tutorial

Follow [Diátaxis tutorials](https://diataxis.fr/tutorials/): a lesson the
reader completes with you, where every step produces a result they can see.

- Open with what we will build: "In this tutorial we will…", then the
  prerequisites, set up in the page itself.
- Each tutorial stands alone. Build everything from scratch in the page, or
  install it from a public repository (the book's examples live in
  `submilli/acme`); never ask the reader to clone this repository or to have
  done another tutorial.
- Give one path. No options, alternatives, or "you could also"; those are
  how-tos.
- Concrete steps, each followed by its real output. Point at what matters with
  "Notice that…". Keep explanation to a sentence and link to the page that
  explains.
- Show whole files the reader saves, not fragments, unless the step changes
  one line of a file already shown.
- Close with one line on what the reader built and where to go next.
- For runs with a model, show one real run and tell the reader to watch for
  its shape, not its text.

### Reference

Describe the machinery and nothing else: every field, flag, limit, and error,
structured like the thing it describes. Caveats, edge cases, and full option
lists live here, so the other types can stay short.

What the binaries can print, they print: the CLI's help, the standard
library's functions and types, the built-ins, and the capabilities and their
fields are generated into the pages between `<!-- generated:NAME -->` and
`<!-- /generated:NAME -->` markers. Don't edit inside a region; change the
doc comment or help text in the code, then run `npm run reference` in
`docs-site` with `SUBMILLI_BIN` and `SUBMILLI_SERVER_BIN` naming the freshly
built binaries. Run it before every release.

## Authorship and required disclosure

Every published documentation page must disclose how it was written. Use one
of these labels, based on the actual authoring process:

| Label | Meaning |
| --- | --- |
| Human-written | A person wrote the prose, including its argument, structure, and sentences. AI may check facts, run examples, or identify problems, but its generated wording is not retained. |
| AI-assisted | The prose combines human writing with retained AI-generated or AI-rewritten wording. Human review alone does not establish human authorship. |
| AI-generated | AI produced the prose, with human review or light editing. |
| Generated from source | A deterministic tool produced the content from code, schemas, CLI help, or other maintained source material. This is distinct from AI generation. |

### Explanations and narrative are human-written

A person must write explanations, conceptual introductions, arguments, and
narrative passages intended to help readers understand Submilli. This includes
the explanatory prose in tutorials and how-to guides, not just pages classified
as Explanation. The author owns the reasoning, order, examples, and sentences.

AI may research, check claims against the code, run examples, capture outputs,
and point out unclear passages. The person writes the resulting explanation.
An AI draft or paraphrase must be rewritten by a person before it meets this
requirement. Editing to lower a detector score does not establish human authorship.

### Reference may be AI-generated

Reference material may be AI-generated or AI-assisted. Fields, flags, types,
limits, errors, and lookup tables must still be accurate, complete, and checked
against the implementation. Prefer deterministic generation for facts the
binaries or schemas can provide. Disclose that as Generated from source.

The reference exception applies to factual lookup material. Moving a narrative
explanation into the reference folder does not exempt it from human authorship.

### Disclosure is mandatory

Show the authorship label on every published page. A subtle icon is acceptable
if its meaning is available on hover, keyboard focus, and tap, with an accessible
text label. Include the disclosure in Markdown exports and other published
representations too.

For mixed pages, use AI-assisted when AI-authored prose remains and identify
sections with a different provenance where needed. Identify generated reference
regions separately; generated API tables do not make surrounding human-written
explanations AI-generated.

A person confirms the label against the version being published. Reconfirm it
after content changes. Detector scores can help prioritize editorial review;
they must never assign or certify authorship. Human-written is an explicit
human declaration, not the absence of an AI flag.

For the existing-book migration, the owner has selected AI-assisted as the
starting classification until individual pages are checked. This is a migration
instruction, not an automatic site fallback. Existing pages without metadata or
a visible disclosure remain migration work; this guide does not label them. A
person may explicitly mark a page Human-written after confirming its prose meets
that definition. The migration does not waive the human-writing requirement for
explanations or permit known AI-generated pages to be mislabeled.

Apply this gate to every new or revised page. Before publication, check both
the writing requirement and the disclosure.
Missing or stale disclosure leaves the page incomplete. Disclosure describes
provenance; it is not a claim that the page is accurate or has been reviewed.

### Declare authorship in Markdown

Add `authorship` to the page's existing YAML frontmatter. Keep its `title`,
`description`, `slug`, and other fields. Use exactly one of `human-written`,
`ai-assisted`, `ai-generated`, or `generated-from-source` for `label`.

```yaml
---
title: "Example reference"
description: "Fields and defaults for an example configuration."
slug: reference/example
# Keep the page's other frontmatter fields here.
authorship:
  label: ai-assisted
  confirmed: true
  contentHash: "REPLACE_WITH_SHA256_FROM_THE_COMMAND_BELOW"
  confirmedAt: "REPLACE_WITH_CONFIRMATION_TIME_FROM_THE_COMMAND_BELOW"
---

Authorship: AI-assisted.

The page content starts here.
```

The uppercase placeholders are not valid metadata. Replace them before
publication. `confirmed: true` records a person's explicit confirmation of the
label for this version; an assistant must not set it without that confirmation.
`confirmedAt` is the confirmation time in ISO 8601 format with a timezone.
`contentHash` is the lowercase SHA-256 digest of the Markdown body, excluding
frontmatter, with CRLF converted to LF and surrounding whitespace trimmed.

Include the visible `Authorship: …` line as the first body paragraph. Use the
human-readable label from the table. The current Markdown exporter omits
frontmatter, so this line keeps disclosure in the exported chapter and combined
agent documentation. For mixed content, add an explicit note beside the relevant
section, for example: `Authorship of this table: Generated from source.` Include
these notes before calculating the hash.

After the person confirms the label and the body is final, run this from
`docs-site/` to calculate the values. Replace the example path with the page's
path. The command prints values only; it does not confirm authorship or edit files.

```sh
node --input-type=module - ../docs/part-6-reference/blueprint-file.md <<'JS'
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { parseFrontmatter } from 'astro/markdown';

const { content } = parseFrontmatter(readFileSync(process.argv[2], 'utf8'));
const body = content.replace(/\r\n/g, '\n').trim();
console.log('contentHash:', createHash('sha256').update(body, 'utf8').digest('hex'));
console.log('confirmedAt:', new Date().toISOString());
JS
```

Copy the digest and timestamp into the frontmatter, keeping the timestamp
quoted. Use the actual confirmation time if confirmation happened earlier.
Changing only frontmatter does not change the body hash. After any body edit,
recheck the visible disclosure and obtain confirmation again, then regenerate
the hash and timestamp.

The site displays the authorship icon only when `confirmed` is true and the
hash matches the current body. Missing or stale metadata suppresses the icon;
it does not mean Human-written. The schema currently permits missing metadata,
so build success alone does not satisfy the disclosure requirement. Reviewers
must check both the frontmatter and visible disclosure before publication.

## Prose style

Machine-written prose has a texture readers recognize, and once they
notice it they stop trusting the page. Avoid:

- **Semicolons.** Use two sentences, or join with *and*, *but*, or *so*.
- **Colons that explain.** A colon introduces a list, a code block, or a
  table. "It is suggestible: it must read things" is two sentences.
- **Em dashes in prose.** Use a comma, parentheses, or a full stop.
- **Contrast pairs.** "X, not Y", "rather than", "not only… but". Keep
  one only where a reader would otherwise get it wrong.
- **Cleft sentences.** "A session is what your application opens" is
  "Your application opens a session."
- **Absolutes and intensifiers.** *every*, *whole*, *exactly*, *truly*,
  *crucial*, *robust*, *seamless*, *powerful*, *ensure*, *leverage*.
  Say how many, or say nothing.
- **Bold lead-ins.** A bolded phrase opening every bullet or paragraph.
- **Lists of three by reflex.** Name as many things as there are.
- **The same shape on every page.** Openings, section rhythm, and
  closing lines that read like a template.

Read the page aloud. If it sounds like a press release or a chatbot,
rewrite it in your own words.

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
For the evaluating agent, state each claim explicitly, name the mechanism that
enforces it, and say where it stops applying. An evaluator trusts a stated limit
more than an unqualified claim. Distinguish what Submilli guarantees, what it
leaves to the person configuring it, and what is advice; in language chapters,
distinguish supported syntax from runtime behavior. Use consistent terminology,
and make headings descriptive enough that a reader retrieving one section can
find the relevant rule.

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
[site instructions](../docs-site/README.md#build-and-preview).

Then read the prose aloud. Rewrite anything you would struggle to say to a
colleague. Cut repetition without removing details needed to use the feature.

Before handing a page over for review, check it against this list:

- **Less is better.** Move verified content from existing pages before writing
  new prose, and cut what is true but doesn't advance the page.
- **Every output is real.** Capture it from a scratch run under an isolated
  `SUBMILLI_HOME`; don't copy it from another page. After reordering steps,
  replay the whole sequence. Say what couldn't be run, and why, in the page's
  frontmatter or a Linear issue, not in the prose.
- **Let the tool's output thread the page.** Lint after the step that needs
  it, and name the warning the next step clears.
- **Explain arguments the first time** a command appears, and define a term
  in a sentence or two where the reader first needs it.
- **Real examples, not stubs.** A package makes its HTTP call and reads its
  key; names, paths, and ids stay consistent within the page.
- **Purpose before command,** in the reader's terms and with concrete things:
  "the customer the agent is serving", not "the session's context".
- **No spatial references** ("above", "on the left"); name the thing or link
  to the heading.
- **Links are real.** A sentence about another topic at the end of a section
  is a missing section or a missing link.
- **A cold read helps.** A fresh agent given only the page finds
  curse-of-knowledge gaps; treat its findings as candidates.
