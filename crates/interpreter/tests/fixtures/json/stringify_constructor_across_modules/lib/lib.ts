function text(value: NumberConstructor): string {
  return JSON.stringify(value);
}

function optional(value: NumberConstructor): string | undefined {
  return JSON.stringify(value);
}

export function runText(value: NumberConstructor): string {
  return text(value);
}

export function runOptional(value: NumberConstructor): string | undefined {
  return optional(value);
}
