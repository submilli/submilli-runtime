/** Shows whether a provider key is present and gives the user one clear way to manage it. */
export function ProviderKeyStatus(props: {
  configured: boolean;
  onManage: () => void;
  providerLabel?: string;
}) {
  const keyLabel = props.providerLabel ? `${props.providerLabel} key` : "provider key";

  return (
    <div className={`pg-provider-key-status ${props.configured ? "is-configured" : "is-missing"}`}>
      {props.configured ? (
        <span className="pg-key-status pg-key-status-saved" role="status">
          <svg className="pg-key-status-icon" width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
            <path d="m3 8.2 3.1 3.1L13 4.7" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          <span>Key saved</span>
        </span>
      ) : (
        <span className="pg-key-status pg-key-status-missing" role="status">
          No {keyLabel} saved
        </span>
      )}
      <button
        type="button"
        className={`pg-btn ${props.configured ? "" : "pg-primary"}`.trim()}
        aria-label={props.configured ? `Change ${keyLabel}` : `Add ${keyLabel}`}
        onClick={props.onManage}
      >
        {props.configured ? "Change key" : "Add key"}
      </button>
    </div>
  );
}
