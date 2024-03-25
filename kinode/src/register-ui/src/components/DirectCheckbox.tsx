interface Props {
  direct: boolean;
  setDirect: (direct: boolean) => void;
}

export default function DirectCheckbox({ direct, setDirect }: Props) {
  return (
    <div className="row">
      <div style={{ position: "relative" }}>
        <input
          type="checkbox"
          id="direct"
          name="direct"
          checked={direct}
          onChange={(e) => setDirect(e.target.checked)}
          autoFocus
        />
        {direct && (
          <span onClick={() => setDirect(false)} className="checkmark">
            &#10003;
          </span>
        )}
      </div>
      <label htmlFor="direct" className="direct-node-message">
        Register as a direct node. If you are unsure leave unchecked.
      </label>
      <div className="tooltip-container">
        <div className="tooltip-button">&#8505;</div>
        <div className="tooltip-content">
          A direct node publishes its own networking information on-chain: IP,
          port, so on. An indirect node relies on the service of routers, which
          are themselves direct nodes. Only register a direct node if you know
          what you’re doing and have a public, static IP address.
        </div>
      </div>
    </div>
  );
}
