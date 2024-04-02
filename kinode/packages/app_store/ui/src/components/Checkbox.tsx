import React from "react";

export default function Checkbox({
  readOnly = false,
  checked,
  setChecked,
}: {
  readOnly?: boolean;
  checked: boolean;
  setChecked?: (checked: boolean) => void;
}) {
  return (
    <div style={{ position: "relative" }}>
      <input
        type="checkbox"
        id="checked"
        name="checked"
        checked={checked}
        onChange={(e) => setChecked && setChecked(e.target.checked)}
        autoFocus
        readOnly={readOnly}
      />
      {checked && (
        <span onClick={() => setChecked && setChecked(false)} className="checkmark">
          &#10003;
        </span>
      )}
    </div>
  );
}
