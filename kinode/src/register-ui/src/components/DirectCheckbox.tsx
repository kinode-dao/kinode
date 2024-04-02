import { DirectTooltip } from "./DirectTooltip";
import { Tooltip } from "./Tooltip";

interface Props {
  direct: boolean;
  setDirect: (direct: boolean) => void;
}

export default function DirectCheckbox({ direct, setDirect }: Props) {
  return (
    <div className="flex place-items-center">
      <div className="relative flex place-items-center mr-2">
        <input
          type="checkbox"
          id="direct"
          name="direct"
          checked={direct}
          onChange={(e) => setDirect(e.target.checked)}
          autoFocus
        />
        {direct && (
          <span
            onClick={() => setDirect(false)}
            className="checkmark"
          >
            &#10003;
          </span>
        )}
      </div>
      <label
        htmlFor="direct"
        className="flex place-items-center cursor-pointer"
      >
        Register as a direct node. If you are unsure leave unchecked.
      </label>
      <DirectTooltip />
    </div>
  );
}
