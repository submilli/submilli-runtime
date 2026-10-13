import { useEffect, useState } from "react";

import { loadStatus } from "./session";

export function App() {
  const [text, setText] = useState("Signing in...");
  useEffect(() => {
    loadStatus().then(setText, (error: unknown) => setText(String(error)));
  }, []);
  return (
    <main>
      <h1>Submilli playground</h1>
      <pre>{text}</pre>
    </main>
  );
}
