import { DirectTooltip } from "./DirectTooltip";

interface Props {
  direct: boolean;
  setDirect: (direct: boolean) => void;
}

export default function DirectCheckbox({ direct, setDirect }: Props) {
  return (
    <div className="direct-checkbox">
      <label className="checkbox-container">
        <input
          type="checkbox"
          checked={direct}
          onChange={(e) => setDirect(e.target.checked)}
        />
        <span className="checkmark"></span>
        <span className="checkbox-label">
          Register as a direct node. If you are unsure leave unchecked.
        </span>
      </label>
      <DirectTooltip />
    </div>
  );
}