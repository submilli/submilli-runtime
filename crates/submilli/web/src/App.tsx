// The page's top: the signed-out screen, the first load, or the page itself.
import { SIGNED_OUT_LINE } from "./api/login";
import type { PageStore } from "./store/store";
import { useStore } from "./store/useStore";
import { Page } from "./ui/Page";
import { LOADING, loadFailed } from "./ui/wording";

function Screen({ line }: { line: string }) {
  return (
    <div className="pg pg-screen">
      <div className="pg-brand">
        Submilli <span>Playground</span>
      </div>
      <p>{line}</p>
    </div>
  );
}

export function App({ store }: { store: PageStore }) {
  const snap = useStore(store);
  if (snap.auth === "signed-out") return <Screen line={SIGNED_OUT_LINE} />;
  if (!snap.data.status) return <Screen line={snap.loadError ? loadFailed(snap.loadError) : LOADING} />;
  return <Page store={store} snap={snap} />;
}
