import llm, { Model } from "submilli:llm";

function main(): Model[] {
  const models: Model[] = llm.models();
  assert(models.length > 0, "the fixture provider supplies at least one model");
  return models;
}
