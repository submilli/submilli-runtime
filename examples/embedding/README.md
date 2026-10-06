# Embedding example

A program that embeds three support notes as documents and one question as a
query, ranks the notes by cosine similarity, and prints the embedding-space
identity of the vectors. The
[Blueprint file reference](/docs/reference/blueprint-file#embedding) describes
the policy and the [standard library reference](/docs/reference/standard-library#embeddings)
describes the module.

| File | What it is |
|:-----|:-----------|
| `blueprint.yaml` | The policy: a Voyage provider, one alias `notes-embedding`, and `embedding.embed` granted for aliases named `notes-*`. |
| `search-notes.ts` | The program: `embed` the notes as `"document"`, `embed` the question as `"query"`, rank by the dot product of `vector(i)`. |

## Running it

The provider's key is a secret of yours. Put it in the local secret store, then
run the program under the blueprint:

```sh
submilli secret put voyage_api_key
submilli run --blueprint blueprint.yaml search-notes.ts
```

`submilli check search-notes.ts` typechecks the program without a key.

The program returns its output as an array of lines. The first line is
`identity: ` followed by the alias's identity, a string beginning `emb1:voyage:voyage-3.5:1024:`
and ending in a digest of the alias's configuration. The next three lines are the notes,
best match first, each a similarity score to three decimals and the note's text. A real run
printed:

```text
["identity: emb1:voyage:voyage-3.5:1024:cc25df8731a6a176dd70220bb0abae43","0.565  Refunds are issued to the original payment method within five business days.","0.361  Invoices are generated on the first day of each month and emailed to the billing contact.","0.318  The dashboard supports a dark theme under Settings, then Appearance."]
```

The refund note ranks first for the question "How do I get my money back?". Scores
can vary slightly between runs and models.

To use another provider, change the `type`, `model`, and `dimensions` of the alias
in `blueprint.yaml`; the program doesn't change.

Without a valid key the program stops at its first `embed` call with an error
that the provider rejected the credential.
