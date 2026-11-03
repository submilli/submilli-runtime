import llm from "submilli:llm";

function main(): void {
  const first = llm.batch("claude-haiku-4-5", ["hello"])[0];
  let text = "";
  if (first.text !== undefined) text = first.text;
  assert(text.startsWith("answer 0"));
  let tokens = 0;
  if (first.inputTokens !== undefined) tokens += first.inputTokens;
  assert(tokens >= 0);
  const models = llm.models();
  const model = models[0];
  if (model.description !== undefined) {
    const description: string = model.description;
    assert(description.length > 0);
  }
  if (model.contextWindow !== undefined) {
    const context: number = model.contextWindow;
    assert(context === 200000);
  }
  let missing = "unset";
  const bare = models[1];
  if (bare.description !== undefined) missing = bare.description;
  assert(missing === "unset");
  const wrapped = { model };
  if (wrapped.model.description !== undefined) {
    const description: string = wrapped.model.description;
    assert(description === "Cheap and fast.");
  }
  assert(model.description !== undefined && model.description.startsWith("Cheap"));
  assert(model.description === undefined ? false : model.description.length > 0);
}
