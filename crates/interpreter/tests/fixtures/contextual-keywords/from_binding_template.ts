interface Header {
  value: string;
}

function findHeader(name: string): Header | null {
  if (name === "From") {
    return { value: "alice@example.com" };
  }
  return null;
}

function main(): void {
  const fromHeader = findHeader("From");
  const subjectHeader = findHeader("Subject");
  const from = fromHeader !== null ? fromHeader.value : "Unknown Sender";
  const subject = subjectHeader !== null ? subjectHeader.value : "No Subject";
  const summary = `From: ${from}
Subject: ${subject}`;
  assert(summary === "From: alice@example.com\nSubject: No Subject");
}
