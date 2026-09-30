// A Vercel AI SDK agent that runs its programs on submilli-server, over HTTP.

import { google } from "@ai-sdk/google";
import { generateText, stepCountIs, type LanguageModel } from "ai";
import { openSession } from "./submilli.ts";

const SUBMILLI_SERVER = process.env.SUBMILLI_SERVER ?? "http://127.0.0.1:8128";

function instructions(userId: string): string {
  return [
    "You are a research assistant. Search the web and read pages by writing programs for Submilli.",
    "Do the whole job in one program where you can, and return only what you need to answer.",
    `Keep a note of what you learn, with its sources, under /${userId}/notes.`,
    "Read your earlier notes before you search again.",
  ].join(" ");
}

export async function answer(
  question: string,
  userId: string,
  model: LanguageModel = google("gemini-3.8-flash"),
): Promise<string> {
  const submilli = await openSession({
    server: SUBMILLI_SERVER,
    token: serverToken(),
    blueprint: "research",
    variables: { userId },
  });

  try {
    const { text } = await generateText({
      model,
      system: instructions(userId),
      tools: submilli.tools,
      prompt: question,
      stopWhen: stepCountIs(20),
    });
    return text;
  } finally {
    await submilli.close();
  }
}

/** The API token this application was given for the server. */
function serverToken(): string {
  const token = process.env.SUBMILLI_SERVER_TOKEN;
  if (!token) throw new Error("SUBMILLI_SERVER_TOKEN is not set: export the token the server was started with");
  return token;
}

if (import.meta.filename === process.argv[1]) {
  // In a real application the user comes from the signed-in session.
  console.log(await answer("What is new in the latest stable release of Rust? Save a note with your sources.", "u_ada"));
}
